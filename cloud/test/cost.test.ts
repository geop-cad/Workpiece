import assert from "node:assert/strict";
import { test } from "node:test";
import { COUNT_MARGIN_RATIO, COUNT_MARGIN_TOKENS, MAX_INPUT_TOKENS, MAX_TOKENS, PRICES } from "../lib/agent/config.ts";
import { costOf, inputBound, inputLimit, reservationFor } from "../lib/agent/cost.ts";

const sonnet = PRICES["claude-sonnet-5-5"];

test("prices are nanodollars per token", () => {
  assert.deepEqual(sonnet, { input: 2000, cacheWrite5m: 2500, cacheWrite1h: 4000, cacheRead: 100, output: 10000 });
  assert.deepEqual(PRICES["claude-opus-5-5"], { input: 4000, cacheWrite5m: 5000, cacheWrite1h: 8000, cacheRead: 200, output: 20000 });
  assert.equal(PRICES["claude-haiku-5-5"].input, 100);
});

test("cost of a usage, every field billed at its rate", () => {
  // 1M plain input = $2, 1M cache read = $0.10, 1M written = $2.50, 1M output = $10.
  const usage = { input_tokens: 1e6, cache_read_input_tokens: 1e6, cache_creation_input_tokens: 1e6, output_tokens: 1e6 };
  assert.equal(costOf(sonnet, usage), (2 + 0.1 + 2.5 + 10) * 1e9);
});

test("1-hour cache writes are priced at 2x when the breakdown says so", () => {
  const usage = {
    input_tokens: 0,
    output_tokens: 0,
    cache_creation_input_tokens: 300,
    cache_creation: { ephemeral_5m_input_tokens: 100, ephemeral_1h_input_tokens: 200 },
  };
  assert.equal(costOf(sonnet, usage), 100 * 2500 + 200 * 4000);
});

test("null and missing usage fields cost nothing", () => {
  assert.equal(costOf(sonnet, { input_tokens: 10, output_tokens: 1, cache_read_input_tokens: null, cache_creation_input_tokens: null }), 10 * 2000 + 10000);
});

test("the reservation bounds every split of the counted input, with max_tokens of output", () => {
  const counted = 20_000;
  const reserved = reservationFor(sonnet, counted, MAX_TOKENS);
  const bound = Math.ceil(counted * (1 + COUNT_MARGIN_RATIO)) + COUNT_MARGIN_TOKENS;
  for (const [plain, written, read] of [[bound, 0, 0], [0, bound, 0], [0, 0, bound], [bound / 2, bound / 4, bound / 4]]) {
    const cost = costOf(sonnet, { input_tokens: plain, cache_creation_input_tokens: written, cache_read_input_tokens: read, output_tokens: MAX_TOKENS });
    assert.ok(cost <= reserved, `${cost} > ${reserved}`);
  }
  assert.equal(reserved, bound * 2500 + MAX_TOKENS * 10000);
});

test("the margined bound, not the bare count, is held to the limit, and never into a long-context band", () => {
  assert.equal(inputLimit(sonnet), MAX_INPUT_TOKENS);
  for (const price of Object.values(PRICES)) assert.ok(inputLimit(price) <= (price.longContextAbove ?? Infinity));
  // A count just under the limit has a bound over it.
  const counted = MAX_INPUT_TOKENS - 100;
  assert.ok(inputBound(counted) > inputLimit(sonnet));
});

test("a model with a long-context band is priced in it above its threshold", () => {
  const haiku = PRICES["claude-haiku-5-5"];
  assert.equal(haiku.longContextAbove, 100_000);
  const short = { input_tokens: 50_000, output_tokens: 1000, cache_read_input_tokens: 50_000 };
  assert.equal(costOf(haiku, short), 50_000 * 100 + 50_000 * 10 + 1000 * 500);
  // Over 100K prompt tokens in all (plain + written + read): every token at $0.50 / $2.50.
  const long = { input_tokens: 60_000, output_tokens: 1000, cache_read_input_tokens: 50_000 };
  assert.equal(costOf(haiku, long), 60_000 * 500 + 50_000 * 50 + 1000 * 2500);
  assert.equal(reservationFor(haiku, 200_000, 1000), inputBound(200_000) * 625 + 1000 * 2500);
});
