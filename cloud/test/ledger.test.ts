import assert from "node:assert/strict";
import { test } from "node:test";
import {
  GLOBAL_TURNS_PER_MINUTE,
  IP_NANO_USD_PER_DAY,
  IP_SESSIONS_PER_DAY,
  IP_TURNS_PER_DAY,
  RESERVATION_TTL_MS,
  SESSION_NANO_USD_PER_DAY,
  SESSION_TURNS_PER_DAY,
  SESSION_TURNS_PER_MINUTE,
} from "../lib/agent/config.ts";
import { admit, available, ledger, openSession, reserve, settle, type Budgets } from "../lib/agent/ledger.ts";
import { MemoryKv } from "./memory-kv.ts";

const USD = 1e9;
const budgets: Budgets = { daily: 20 * USD, total: 100 * USD };
const t0 = Date.parse("2026-10-25T12:00:00Z");
const MIN = 60_000;
let n = 0;
const id = () => `r${n++}`;

/** One whole turn, admitted, reserved and settled; spaced so no rate limit is hit. */
function turn(kv: MemoryKv, b: Budgets, sid: string, ip: string, now: number, amount: number, cost: number) {
  const a = admit(kv, b, { sid, ip, now });
  if (!a.ok) return a;
  const rid = id();
  const r = reserve(kv, b, { sid, ip, now, id: rid, amount });
  if (r.ok) settle(kv, { id: rid, cost, now });
  return r;
}

test("reserve before, settle after: the day and the total carry the actual cost", () => {
  const kv = new MemoryKv();
  assert.ok(admit(kv, budgets, { sid: "s", ip: "i", now: t0 }).ok);
  const r = reserve(kv, budgets, { sid: "s", ip: "i", now: t0, id: "a", amount: 0.5 * USD });
  assert.ok(r.ok);
  assert.deepEqual(r.limits, { turns_left_today: SESSION_TURNS_PER_DAY - 1, turns_per_day: SESSION_TURNS_PER_DAY });
  // While open, the reservation counts against the remainder.
  assert.equal(ledger(kv).total, 0);
  settle(kv, { id: "a", cost: 0.25 * USD, now: t0 + 1000 });
  const l = ledger(kv);
  assert.equal(l.total, 0.25 * USD);
  assert.deepEqual(l.days["2026-10-25"], { spent: 0.25 * USD, turns: 1, sessions: 0, swept: 0 });
});

test("a reservation that does not fit the day's remainder is refused, open reservations counted", () => {
  const kv = new MemoryKv();
  const small: Budgets = { daily: 1 * USD, total: 100 * USD };
  assert.ok(reserve(kv, small, { sid: "a", ip: "1", now: t0, id: "x", amount: 0.6 * USD }).ok);
  const refused = reserve(kv, small, { sid: "b", ip: "2", now: t0, id: "y", amount: 0.6 * USD });
  assert.equal(refused.ok, false);
  assert.ok(!refused.ok && refused.error === "budget" && refused.retry_after === 12 * 3600);
  // Settling below the reservation frees the rest.
  settle(kv, { id: "x", cost: 0.1 * USD, now: t0 });
  assert.ok(reserve(kv, small, { sid: "b", ip: "2", now: t0, id: "y", amount: 0.6 * USD }).ok);
});

test("the daily cap resets at 00:00 UTC; the total never does", () => {
  const kv = new MemoryKv();
  const b: Budgets = { daily: 1 * USD, total: 1.5 * USD };
  assert.ok(turn(kv, b, "s0", "i0", t0, 0.5 * USD, 0.5 * USD).ok);
  assert.ok(turn(kv, b, "s1", "i1", t0, 0.5 * USD, 0.5 * USD).ok);
  const sameDay = admit(kv, b, { sid: "s2", ip: "i2", now: t0 + MIN });
  assert.ok(!sameDay.ok && sameDay.error === "budget" && sameDay.retry_after !== undefined);
  const nextDay = t0 + 24 * 3600 * 1000;
  assert.ok(turn(kv, b, "s", "i", nextDay, 0.5 * USD, 0.5 * USD).ok);
  // 1.5 of 1.5 spent over two days: paused for good, with no time to come back.
  const after = admit(kv, b, { sid: "s3", ip: "i3", now: nextDay + 2 * 24 * 3600 * 1000 });
  assert.ok(!after.ok && after.error === "budget" && after.retry_after === undefined);
  assert.equal(available(kv, b, nextDay + 3 * 24 * 3600 * 1000), false);
  // Raising the total (the var) is all it takes to recharge.
  assert.equal(available(kv, { daily: 1 * USD, total: 2 * USD }, nextDay + 3 * 24 * 3600 * 1000), true);
  assert.equal(ledger(kv).total, 1.5 * USD);
});

test("caps of zero (a missing var) refuse everything", () => {
  const kv = new MemoryKv();
  const closed: Budgets = { daily: 0, total: 0 };
  assert.equal(available(kv, closed, t0), false);
  assert.ok(!admit(kv, closed, { sid: "s", ip: "i", now: t0 }).ok);
  assert.ok(!openSession(kv, closed, { sid: "s", ip: "i", now: t0 }).ok);
});

test("a reservation never settled is charged in full after its TTL, and a late settle is not charged again", () => {
  const kv = new MemoryKv();
  assert.ok(reserve(kv, budgets, { sid: "s", ip: "i", now: t0, id: "lost", amount: 0.3 * USD }).ok);
  available(kv, budgets, t0 + RESERVATION_TTL_MS + 1);
  assert.equal(ledger(kv).total, 0.3 * USD);
  assert.equal(ledger(kv).days["2026-10-25"].swept, 1);
  settle(kv, { id: "lost", cost: 0.1 * USD, now: t0 + RESERVATION_TTL_MS + 2 });
  assert.equal(ledger(kv).total, 0.3 * USD);
});

test("a settle with no reservation on record is charged", () => {
  const kv = new MemoryKv();
  settle(kv, { id: "unknown", cost: 5, now: t0 });
  assert.equal(ledger(kv).total, 5);
});

test("per-session minute rate", () => {
  const kv = new MemoryKv();
  for (let i = 0; i < SESSION_TURNS_PER_MINUTE; i++) assert.ok(admit(kv, budgets, { sid: "s", ip: "i", now: t0 + i }).ok);
  const refused = admit(kv, budgets, { sid: "s", ip: "i", now: t0 + 1000 });
  assert.ok(!refused.ok && refused.error === "rate" && refused.retry_after === 59);
  // Another session from the same address is not limited by this one's rate.
  assert.ok(admit(kv, budgets, { sid: "t", ip: "i", now: t0 + 1000 }).ok);
  // A minute after the first, one slot is free again.
  assert.ok(admit(kv, budgets, { sid: "s", ip: "i", now: t0 + MIN }).ok);
});

test("global minute rate holds over many addresses, counting reserved turns only", () => {
  const kv = new MemoryKv();
  // Admitted but never reserved (refused at count_tokens, say): everyone's window stays free.
  for (let i = 0; i < 2 * GLOBAL_TURNS_PER_MINUTE; i++) assert.ok(admit(kv, budgets, { sid: `a${i}`, ip: `ipa${i}`, now: t0 }).ok);
  for (let i = 0; i < GLOBAL_TURNS_PER_MINUTE; i++) {
    assert.ok(admit(kv, budgets, { sid: `s${i}`, ip: `ip${i}`, now: t0 }).ok);
    assert.ok(reserve(kv, budgets, { sid: `s${i}`, ip: `ip${i}`, now: t0, id: id(), amount: 1000 }).ok);
  }
  const refused = admit(kv, budgets, { sid: "x", ip: "x", now: t0 });
  assert.ok(!refused.ok && refused.error === "rate" && /busy/.test(refused.message));
  // A turn admitted just before the window filled is refused at reserve.
  const late = reserve(kv, budgets, { sid: "a0", ip: "ipa0", now: t0, id: id(), amount: 1000 });
  assert.ok(!late.ok && late.error === "rate");
});

test("a request refused after admit still counts against the caller's own minute window and day", () => {
  const kv = new MemoryKv();
  for (let i = 0; i < SESSION_TURNS_PER_MINUTE; i++) assert.ok(admit(kv, budgets, { sid: "s", ip: "i", now: t0 }).ok);
  assert.ok(!admit(kv, budgets, { sid: "s", ip: "i", now: t0 }).ok);
  const later = admit(kv, budgets, { sid: "s", ip: "i", now: t0 + MIN });
  assert.ok(later.ok && later.limits.turns_left_today === SESSION_TURNS_PER_DAY - SESSION_TURNS_PER_MINUTE - 1);
});

test("money per session and per address per day: the reservation must fit, the settled cost is what counts", () => {
  const kv = new MemoryKv();
  const reserveFor = (sid: string, ip: string, amount: number, now = t0) => {
    const a = admit(kv, budgets, { sid, ip, now });
    if (!a.ok) return a;
    return reserve(kv, budgets, { sid, ip, now, id: id(), amount });
  };
  // One session: its reservations open count; a settled one counts what it cost.
  const first = id();
  assert.ok(admit(kv, budgets, { sid: "s", ip: "i", now: t0 }).ok);
  assert.ok(reserve(kv, budgets, { sid: "s", ip: "i", now: t0, id: first, amount: SESSION_NANO_USD_PER_DAY / 2 }).ok);
  const over = reserveFor("s", "i", SESSION_NANO_USD_PER_DAY / 2 + 1);
  assert.ok(!over.ok && over.error === "quota");
  settle(kv, { id: first, cost: SESSION_NANO_USD_PER_DAY / 4, now: t0 });
  assert.ok(reserveFor("s", "i", SESSION_NANO_USD_PER_DAY / 2, t0 + 1).ok);
  // The address: sessions together never pass IP_NANO_USD_PER_DAY.
  let spent = 0;
  for (let i = 0; spent + SESSION_NANO_USD_PER_DAY / 2 <= IP_NANO_USD_PER_DAY; i++) {
    const r = reserveFor(`t${i}`, "j", SESSION_NANO_USD_PER_DAY / 2, t0 + i * MIN);
    assert.ok(r.ok);
    spent += SESSION_NANO_USD_PER_DAY / 2;
  }
  const fresh = reserveFor("fresh", "j", 1, t0 + 10 * MIN);
  assert.ok(!fresh.ok && fresh.error === "quota");
  // A spent address shows no turns left, and admits nothing more today.
  assert.ok(!admit(kv, budgets, { sid: "fresh2", ip: "j", now: t0 + 11 * MIN }).ok);
  // The next UTC day, all is renewed.
  assert.ok(reserveFor("fresh", "j", SESSION_NANO_USD_PER_DAY / 2, t0 + 24 * 3600 * 1000).ok);
});

test("a reservation swept in full counts against the caller's money", () => {
  const kv = new MemoryKv();
  assert.ok(reserve(kv, budgets, { sid: "s", ip: "i", now: t0, id: "gone", amount: SESSION_NANO_USD_PER_DAY }).ok);
  const after = t0 + RESERVATION_TTL_MS + 1;
  const refused = admit(kv, budgets, { sid: "s", ip: "i", now: after });
  assert.ok(!refused.ok && refused.error === "quota");
  assert.equal(ledger(kv).days["2026-10-25"].swept, 1);
});

test("per-session daily quota, renewed the next UTC day", () => {
  const kv = new MemoryKv();
  for (let i = 0; i < SESSION_TURNS_PER_DAY; i++) assert.ok(turn(kv, budgets, "s", "i", t0 + i * MIN, 1000, 10).ok);
  const refused = admit(kv, budgets, { sid: "s", ip: "i", now: t0 + SESSION_TURNS_PER_DAY * MIN });
  assert.ok(!refused.ok && refused.error === "quota");
  assert.ok(turn(kv, budgets, "s", "i", t0 + 24 * 3600 * 1000, 1000, 10).ok);
});

test("per-address daily quota spans its sessions", () => {
  const kv = new MemoryKv();
  for (let i = 0; i < IP_TURNS_PER_DAY; i++) assert.ok(turn(kv, budgets, `s${Math.floor(i / 30)}`, "ip", t0 + i * MIN, 1000, 10).ok);
  const refused = admit(kv, budgets, { sid: "fresh", ip: "ip", now: t0 + IP_TURNS_PER_DAY * MIN });
  assert.ok(!refused.ok && refused.error === "quota");
});

test("two turns racing cannot both take the session's last turn, or its last money", () => {
  const kv = new MemoryKv();
  for (let i = 0; i < SESSION_TURNS_PER_DAY - 1; i++) assert.ok(turn(kv, budgets, "s", "i", t0 + i * MIN, 1000, 10).ok);
  const now = t0 + SESSION_TURNS_PER_DAY * MIN;
  assert.ok(admit(kv, budgets, { sid: "s", ip: "i", now }).ok);
  const second = admit(kv, budgets, { sid: "s", ip: "i", now });
  assert.ok(!second.ok && second.error === "quota");
  // Money: two admitted turns of one session, the second's reservation does not fit beside the first's.
  assert.ok(admit(kv, budgets, { sid: "m", ip: "k", now }).ok);
  assert.ok(admit(kv, budgets, { sid: "m", ip: "k", now }).ok);
  assert.ok(reserve(kv, budgets, { sid: "m", ip: "k", now, id: "first", amount: 0.4 * USD }).ok);
  const racing = reserve(kv, budgets, { sid: "m", ip: "k", now, id: "second", amount: 0.4 * USD });
  assert.ok(!racing.ok && racing.error === "quota");
});

test("sessions per address per day", () => {
  const kv = new MemoryKv();
  for (let i = 0; i < IP_SESSIONS_PER_DAY; i++) assert.ok(openSession(kv, budgets, { sid: `s${i}`, ip: "ip", now: t0 }).ok);
  const refused = openSession(kv, budgets, { sid: "more", ip: "ip", now: t0 });
  assert.ok(!refused.ok && refused.error === "rate");
  assert.ok(openSession(kv, budgets, { sid: "other", ip: "ip2", now: t0 }).ok);
  assert.ok(openSession(kv, budgets, { sid: "tomorrow", ip: "ip", now: t0 + 24 * 3600 * 1000 }).ok);
  assert.equal(ledger(kv).days["2026-10-25"].sessions, IP_SESSIONS_PER_DAY + 1);
});

test("concurrent reservations never let settled spend pass the cap", () => {
  // Many turns reserving at once, each settling at most its reservation:
  // whatever order they settle in, the day never goes over.
  const kv = new MemoryKv();
  const b: Budgets = { daily: 1 * USD, total: 100 * USD };
  const open: string[] = [];
  for (let i = 0; i < 50; i++) {
    const rid = `c${i}`;
    if (reserve(kv, b, { sid: `s${i}`, ip: `i${i}`, now: t0, id: rid, amount: 0.15 * USD }).ok) open.push(rid);
  }
  assert.equal(open.length, 6);
  for (const rid of open) settle(kv, { id: rid, cost: 0.15 * USD, now: t0 });
  assert.ok(ledger(kv).days["2026-10-25"].spent <= b.daily);
});

test("status says unavailable once no turn's least reservation fits", () => {
  const kv = new MemoryKv();
  const b: Budgets = { daily: 1 * USD, total: 100 * USD };
  assert.ok(turn(kv, b, "s0", "i0", t0, 0.5 * USD, 0.5 * USD).ok);
  assert.ok(turn(kv, b, "s1", "i1", t0, 0.45 * USD, 0.45 * USD).ok);
  assert.equal(available(kv, b, t0, 0.01 * USD), true);
  assert.equal(available(kv, b, t0, 0.1 * USD), false);
});
