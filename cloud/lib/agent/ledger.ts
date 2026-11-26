/**
 * The agent chat's accounting: money, turns and rates, over a synchronous
 * key-value store. The budget worker's Durable Object runs these
 * functions on its storage (`ctx.storage.kv`), one call at a time, so
 * every check-and-update here is atomic: a cap checked is a cap held.
 *
 * Money is in nanodollars (integers). A turn first `admit`s (the caller's
 * rates and quotas, before anything is asked of the API: the turn counts
 * against the caller's minute windows and day from here on), then
 * `reserve`s an upper bound of its cost (refused if the bound does not fit
 * the day's or the lifetime's remainder, or the session's or the address's
 * money for the day, reservations still open counted as spent; only a
 * reserved turn counts towards the global minute window), then `settle`s
 * to what the response's usage says it cost. A reservation never settled
 * is charged in full once RESERVATION_TTL_MS has passed.
 *
 * Keys:
 *   total              lifetime spend, never reset
 *   day:<YYYY-MM-DD>   the day's ledger (DayRecord), never deleted
 *   res:<id>           an open reservation (Reservation)
 *   cnt:<key>          a counter of the day in `counters_day` (turns, sessions, money of a session or address)
 *   win:<key>          the timestamps of the last minute's turns
 *   swept:<id>         a reservation charged in full, so a late settle is not charged again
 */
import {
  GLOBAL_SESSIONS_PER_DAY,
  GLOBAL_TURNS_PER_MINUTE,
  IP_NANO_USD_PER_DAY,
  IP_SESSIONS_PER_DAY,
  IP_TURNS_PER_DAY,
  IP_TURNS_PER_MINUTE,
  RESERVATION_TTL_MS,
  SESSION_NANO_USD_PER_DAY,
  SESSION_TURNS_PER_DAY,
  SESSION_TURNS_PER_MINUTE,
} from "./config.ts";

export interface Kv {
  get<T>(key: string): T | undefined;
  put<T>(key: string, value: T): void;
  delete(key: string): boolean;
  list<T>(options: { prefix: string }): Iterable<[string, T]>;
}

/** The caps in nanodollars; 0 for a cap that is missing or unreadable, which closes the endpoint. */
export interface Budgets {
  daily: number;
  total: number;
}

export interface DayRecord {
  /** Settled spend, plus reservations charged in full. */
  spent: number;
  turns: number;
  sessions: number;
  /** Reservations charged in full because they were never settled. */
  swept: number;
}

interface Reservation {
  amount: number;
  day: string;
  at: number;
  sid: string;
  ip: string;
}

export interface Limits {
  turns_left_today: number;
  turns_per_day: number;
}

export type Refusal = {
  ok: false;
  error: "rate" | "quota" | "budget";
  message: string;
  retry_after?: number;
};
export type Granted = { ok: true; limits: Limits };

const MINUTE_MS = 60_000;

export function dayOf(now: number): string {
  return new Date(now).toISOString().slice(0, 10);
}

function secondsToMidnight(now: number): number {
  const midnight = new Date(dayOf(now) + "T00:00:00Z").getTime() + 24 * 60 * 60 * 1000;
  return Math.ceil((midnight - now) / 1000);
}

function dayRecord(kv: Kv, day: string): DayRecord {
  return kv.get<DayRecord>(`day:${day}`) ?? { spent: 0, turns: 0, sessions: 0, swept: 0 };
}

function updateDay(kv: Kv, day: string, change: (record: DayRecord) => void) {
  const record = dayRecord(kv, day);
  change(record);
  kv.put(`day:${day}`, record);
}

function charge(kv: Kv, day: string, amount: number, swept: boolean) {
  kv.put("total", (kv.get<number>("total") ?? 0) + amount);
  updateDay(kv, day, (record) => {
    record.spent += amount;
    if (swept) record.swept += 1;
  });
}

/** Charges a closed reservation: to the day, and to its session's and address's money if the counters are still of its day. */
function chargeReservation(kv: Kv, reservation: Reservation, amount: number, swept: boolean) {
  charge(kv, reservation.day, amount, swept);
  if (kv.get<string>("counters_day") === reservation.day) {
    add(kv, `usd:s:${reservation.sid}`, amount);
    add(kv, `usd:ip:${reservation.ip}`, amount);
  }
}

/** Charges reservations past their TTL in full, and drops the counters and windows of past days. */
function housekeep(kv: Kv, now: number) {
  for (const [key, reservation] of [...kv.list<Reservation>({ prefix: "res:" })]) {
    if (now - reservation.at > RESERVATION_TTL_MS) {
      kv.delete(key);
      kv.put(`swept:${key.slice(4)}`, true);
      chargeReservation(kv, reservation, reservation.amount, true);
    }
  }
  const today = dayOf(now);
  if (kv.get<string>("counters_day") !== today) {
    for (const prefix of ["cnt:", "win:"]) {
      for (const [key] of [...kv.list({ prefix })]) kv.delete(key);
    }
    kv.put("counters_day", today);
  }
}

/** What is left of the day's and the lifetime's budget, open reservations counted as spent. */
function remaining(kv: Kv, budgets: Budgets, now: number): { day: number; total: number } {
  const today = dayOf(now);
  let pendingToday = 0;
  let pending = 0;
  for (const [, reservation] of kv.list<Reservation>({ prefix: "res:" })) {
    pending += reservation.amount;
    if (reservation.day === today) pendingToday += reservation.amount;
  }
  return {
    day: budgets.daily - dayRecord(kv, today).spent - pendingToday,
    total: budgets.total - (kv.get<number>("total") ?? 0) - pending,
  };
}

/** Refuses when `amount` does not fit what is left; null when it does. */
function budgetRefusal(kv: Kv, budgets: Budgets, now: number, amount: number): Refusal | null {
  const left = remaining(kv, budgets, now);
  if (left.total < amount || left.total <= 0) {
    return { ok: false, error: "budget", message: "The free assistant is paused: its budget is used up." };
  }
  if (left.day < amount || left.day <= 0) {
    return {
      ok: false,
      error: "budget",
      message: "The free assistant is paused for today. It is back at 00:00 UTC.",
      retry_after: secondsToMidnight(now),
    };
  }
  return null;
}

const counter = (kv: Kv, key: string) => kv.get<number>(`cnt:${key}`) ?? 0;
const add = (kv: Kv, key: string, amount: number) => kv.put(`cnt:${key}`, counter(kv, key) + amount);
const bump = (kv: Kv, key: string) => add(kv, key, 1);

/** What is left today of the session's and of the address's money, today's open reservations counted as spent. */
function moneyLeft(kv: Kv, sid: string, ip: string, now: number): { session: number; ip: number } {
  const today = dayOf(now);
  let session = SESSION_NANO_USD_PER_DAY - counter(kv, `usd:s:${sid}`);
  let address = IP_NANO_USD_PER_DAY - counter(kv, `usd:ip:${ip}`);
  for (const [, reservation] of kv.list<Reservation>({ prefix: "res:" })) {
    if (reservation.day !== today) continue;
    if (reservation.sid === sid) session -= reservation.amount;
    if (reservation.ip === ip) address -= reservation.amount;
  }
  return { session, ip: address };
}

function window(kv: Kv, key: string, now: number): number[] {
  return (kv.get<number[]>(`win:${key}`) ?? []).filter((at) => now - at < MINUTE_MS);
}

/** The caller's turns left today: none once its money for the day is spent. */
export function limitsOf(kv: Kv, sid: string, ip: string, now: number): Limits {
  const money = moneyLeft(kv, sid, ip, now);
  const left =
    money.session <= 0 || money.ip <= 0
      ? 0
      : Math.min(SESSION_TURNS_PER_DAY - counter(kv, `turns:s:${sid}`), IP_TURNS_PER_DAY - counter(kv, `turns:ip:${ip}`));
  return { turns_left_today: Math.max(0, left), turns_per_day: SESSION_TURNS_PER_DAY };
}

function usedUp(now: number): Refusal {
  return {
    ok: false,
    error: "quota",
    message: "You have used today's free assistant allowance. It renews at 00:00 UTC.",
    retry_after: secondsToMidnight(now),
  };
}

/** Refuses when the caller's turns or money for the day are used up. */
function quotaRefusal(kv: Kv, sid: string, ip: string, now: number): Refusal | null {
  return limitsOf(kv, sid, ip, now).turns_left_today > 0 ? null : usedUp(now);
}

/** Refuses when `amount` does not fit what is left of the session's or the address's money today. */
function moneyRefusal(kv: Kv, sid: string, ip: string, now: number, amount: number): Refusal | null {
  const money = moneyLeft(kv, sid, ip, now);
  return money.session < amount || money.ip < amount ? usedUp(now) : null;
}

function rateRefusal(key: string, recent: number[], now: number): Refusal {
  return {
    ok: false,
    error: "rate",
    message: key === "global" ? "The assistant is busy. Try again in a minute." : "Too many requests. Wait a moment.",
    retry_after: Math.max(1, Math.ceil((recent[0] + MINUTE_MS - now) / 1000)),
  };
}

export interface Caller {
  sid: string;
  ip: string;
  now: number;
}

/**
 * Lets a turn ask the API anything (even to count its tokens): refuses
 * when the budget is gone, the caller's turns or money for the day are
 * used, or a minute rate (the caller's, or everyone's) is reached. From
 * here on the turn counts against the caller's minute windows and turns
 * of the day, whether or not it gets as far as reserving.
 */
export function admit(kv: Kv, budgets: Budgets, { sid, ip, now }: Caller): Granted | Refusal {
  housekeep(kv, now);
  const refusal = budgetRefusal(kv, budgets, now, 0) ?? quotaRefusal(kv, sid, ip, now);
  if (refusal) return refusal;
  const windows: [string, number][] = [
    [`s:${sid}`, SESSION_TURNS_PER_MINUTE],
    [`ip:${ip}`, IP_TURNS_PER_MINUTE],
    ["global", GLOBAL_TURNS_PER_MINUTE],
  ];
  for (const [key, limit] of windows) {
    const recent = window(kv, key, now);
    if (recent.length >= limit) return rateRefusal(key, recent, now);
  }
  for (const key of [`s:${sid}`, `ip:${ip}`]) kv.put(`win:${key}`, [...window(kv, key, now), now]);
  bump(kv, `turns:s:${sid}`);
  bump(kv, `turns:ip:${ip}`);
  return { ok: true, limits: limitsOf(kv, sid, ip, now) };
}

/**
 * Holds `amount` (an upper bound of the turn's cost) under `id`, and
 * counts the turn in everyone's minute window and the day's turns.
 * Refused when the amount does not fit the day's or the lifetime's
 * remainder or the session's or the address's money for the day (two
 * turns racing included), or everyone's minute rate is reached.
 */
export function reserve(kv: Kv, budgets: Budgets, caller: Caller & { id: string; amount: number }): Granted | Refusal {
  const { sid, ip, now, id, amount } = caller;
  housekeep(kv, now);
  const refusal = budgetRefusal(kv, budgets, now, amount) ?? moneyRefusal(kv, sid, ip, now, amount);
  if (refusal) return refusal;
  const recent = window(kv, "global", now);
  if (recent.length >= GLOBAL_TURNS_PER_MINUTE) return rateRefusal("global", recent, now);
  kv.put("win:global", [...recent, now]);
  const day = dayOf(now);
  kv.put<Reservation>(`res:${id}`, { amount, day, at: now, sid, ip });
  updateDay(kv, day, (record) => (record.turns += 1));
  return { ok: true, limits: limitsOf(kv, sid, ip, now) };
}

/**
 * Closes the reservation `id`, charging `cost`, what the turn did cost.
 * One already swept was charged in full and is not charged again; with no
 * record of `id` at all, `cost` is charged to today.
 */
export function settle(kv: Kv, { id, cost, now }: { id: string; cost: number; now: number }) {
  const reservation = kv.get<Reservation>(`res:${id}`);
  if (reservation) {
    kv.delete(`res:${id}`);
    chargeReservation(kv, reservation, cost, false);
  } else if (!kv.get<boolean>(`swept:${id}`)) {
    charge(kv, dayOf(now), cost, false);
  }
  housekeep(kv, now);
}

/** Lets an address open a session (after its Turnstile token was verified). */
export function openSession(kv: Kv, budgets: Budgets, { sid, ip, now }: Caller): Granted | Refusal {
  housekeep(kv, now);
  const refusal = budgetRefusal(kv, budgets, now, 0);
  if (refusal) return refusal;
  const day = dayOf(now);
  if (counter(kv, `sessions:ip:${ip}`) >= IP_SESSIONS_PER_DAY || dayRecord(kv, day).sessions >= GLOBAL_SESSIONS_PER_DAY) {
    return {
      ok: false,
      error: "rate",
      message: "Too many new sessions today. Try again tomorrow.",
      retry_after: secondsToMidnight(now),
    };
  }
  bump(kv, `sessions:ip:${ip}`);
  updateDay(kv, day, (record) => (record.sessions += 1));
  return { ok: true, limits: limitsOf(kv, sid, ip, now) };
}

/** Whether a turn reserving `minimum` (the least any turn reserves) would fit the budget now. Nothing else is said. */
export function available(kv: Kv, budgets: Budgets, now: number, minimum = 0): boolean {
  housekeep(kv, now);
  return budgetRefusal(kv, budgets, now, minimum) === null;
}

/** For the owner: the lifetime spend and every day's record. */
export function ledger(kv: Kv): { total: number; days: Record<string, DayRecord> } {
  const days: Record<string, DayRecord> = {};
  for (const [key, record] of kv.list<DayRecord>({ prefix: "day:" })) days[key.slice(4)] = record;
  return { total: kv.get<number>("total") ?? 0, days };
}
