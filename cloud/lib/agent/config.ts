/**
 * Every tunable of the hosted agent chat in one place. The money caps
 * themselves are not here: they are the budget worker's vars
 * (`AGENT_DAILY_BUDGET_USD`, `AGENT_TOTAL_BUDGET_USD`, see
 * budget-worker/wrangler.toml), so raising one is a var change and a
 * redeploy of that worker, nothing else.
 */

// Per session (one cookie, one Turnstile solve, 24 h).
export const SESSION_TURNS_PER_DAY = 40;
export const SESSION_TURNS_PER_MINUTE = 6;
export const SESSION_LIFETIME_SECONDS = 24 * 60 * 60;

// Per address: CF-Connecting-IP, an IPv4 address whole, an IPv6 address
// by its /64 (one subscriber's network; `clientIp` in http.ts).
export const IP_TURNS_PER_DAY = 80;
export const IP_TURNS_PER_MINUTE = 12;
export const IP_SESSIONS_PER_DAY = 3;

/**
 * Money per UTC day, in nanodollars, per session and per address: what a
 * turn reserves must fit the remainder (open reservations counted as
 * spent), and it is settled to what the turn cost. A typical turn costs
 * 1-4 cents (most of the prompt read from the cache), so a session's
 * 0.75 USD is some 20-40 turns of modelling; the worst turn reserves
 * about 0.19 USD (Sonnet 5.5, MAX_INPUT_TOKENS, MAX_TOKENS). One address
 * takes at most 1.50 USD, 7.5 % of the default 20 USD daily cap.
 */
export const SESSION_NANO_USD_PER_DAY = 750_000_000;
export const IP_NANO_USD_PER_DAY = 1_500_000_000;

// Over everyone. Only turns that reserved count towards the global minute
// window (a request refused before that costs the caller's own windows,
// not everyone's). The global session cap is far above honest use (a few
// hundred people a day) and is there to bound the ledger, not as a lock:
// sessions are scarce per address (IP_SESSIONS_PER_DAY) and the money is
// capped per session, per address and per day.
export const GLOBAL_TURNS_PER_MINUTE = 60;
export const GLOBAL_SESSIONS_PER_DAY = 20_000;

// Request shape.
/**
 * The body without the one image's base64 data. The page compacts its
 * conversation well below this (website/app/assistant/agent.js,
 * COMPACT_AT_BYTES): system and tools are ~6 KB, a long script ~24 KB, a
 * report up to 20 KB, a guide section up to 24 KB.
 */
export const MAX_BODY_BYTES = 128 * 1024;
/** The base64 data of the one image a request may carry (a viewport screenshot). */
export const MAX_IMAGE_BASE64_BYTES = 300 * 1024;
export const MAX_IMAGES_PER_REQUEST = 1;
export const MAX_MESSAGES = 80;
/**
 * A user text block, and the text of a tool result (as a string or each of
 * its text parts): room for a prompt with the note on the open part, for
 * the page's summary of a compacted conversation, and for a tool's answer
 * (the page clips those at 24 000 too). The prompt typed is capped at 4000
 * by the chat box; the cost of a request is bounded by its counted tokens
 * either way (`MAX_INPUT_TOKENS`, the reservation).
 */
export const MAX_USER_TEXT_CHARS = 24_000;
/**
 * Input tokens a request may have: its count with the margin
 * (COUNT_MARGIN_*) must not pass it. System and tools are ~2K tokens, a
 * long script ~8K, a report ~6K, a picture ~0.7K: a working conversation
 * fits, and the page compacts one that grows past it (it answers
 * `too_long` otherwise). Below every long-context price band in PRICES.
 */
export const MAX_INPUT_TOKENS = 40_000;
/** Tool use ids, as the API makes them (`toolu_...`). */
export const TOOL_USE_ID = /^[A-Za-z0-9_-]{1,64}$/;

// The model call.
export const DEFAULT_MODEL = "claude-sonnet-5-5";
/** Thinking counts towards it; a tool-calling turn at medium effort fits comfortably. */
export const MAX_TOKENS = 8192;
export const DEFAULT_EFFORT = "medium";
/** The SDK's own timeout; a reservation outlives it (RESERVATION_TTL_MS). */
export const UPSTREAM_TIMEOUT_MS = 5 * 60 * 1000;

// Accounting.
/**
 * A reservation not settled after this long (its request died between
 * reserving and settling) is charged in full: the API may have billed it.
 * Longer than UPSTREAM_TIMEOUT_MS, so no live request is swept.
 */
export const RESERVATION_TTL_MS = 15 * 60 * 1000;
/**
 * `count_tokens` is documented as an estimate: the reservation takes this
 * share and this many tokens on top of it, so it stays an upper bound.
 */
export const COUNT_MARGIN_RATIO = 0.1;
export const COUNT_MARGIN_TOKENS = 500;

/** Prices in nanodollars (1e-9 USD) per token, which is USD per million tokens times 1000. */
export interface Price {
  input: number;
  /** 5-minute cache writes, 1.25x input. */
  cacheWrite5m: number;
  /** 1-hour cache writes, 2x input (not used by this endpoint, priced in case usage reports them). */
  cacheWrite1h: number;
  cacheRead: number;
  output: number;
}

/** A model's prices, and where it has one, the dearer band a long prompt is billed at. */
export interface ModelPrice extends Price {
  /** Prompts of more input tokens than this are billed at `longContext` (the whole request). */
  longContextAbove?: number;
  longContext?: Price;
}

function price(inputPerMTok: number, cacheReadPerMTok: number, outputPerMTok: number): Price {
  const n = (usdPerMTok: number) => Math.round(usdPerMTok * 1000);
  return {
    input: n(inputPerMTok),
    cacheWrite5m: n(inputPerMTok * 1.25),
    cacheWrite1h: n(inputPerMTok * 2),
    cacheRead: n(cacheReadPerMTok),
    output: n(outputPerMTok),
  };
}

/**
 * Models the endpoint may run; any other `AGENT_MODEL` refuses to start.
 * Anthropic's list prices, checked 2026-11-26 against the claude-api
 * skill's model table (cached 2026-11-03): Opus 5.5 $4 / $20, cache reads
 * $0.20; Sonnet 5.5 $2 / $10, cache reads $0.10; Haiku 5.5 $0.10 / $0.50
 * up to 100K prompt tokens and $0.50 / $2.50 above, cache reads 0.1x
 * input. Opus 5.5 and Sonnet 5.5 have no long-context band.
 */
export const PRICES: Record<string, ModelPrice> = {
  "claude-opus-5-5": price(4, 0.2, 20),
  "claude-sonnet-5-5": price(2, 0.1, 10),
  "claude-haiku-5-5": { ...price(0.1, 0.01, 0.5), longContextAbove: 100_000, longContext: price(0.5, 0.05, 2.5) },
};

export const EFFORTS = ["low", "medium", "high"] as const;
export type Effort = (typeof EFFORTS)[number];

export const NANO_PER_USD = 1e9;
