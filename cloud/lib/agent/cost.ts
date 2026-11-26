/** What a request may cost before it is sent, and what it did cost after. All amounts in nanodollars. */
import { COUNT_MARGIN_RATIO, COUNT_MARGIN_TOKENS, MAX_INPUT_TOKENS, type ModelPrice, type Price } from "./config.ts";

/** The usage fields the Messages API reports, as far as they are billed. */
export interface Usage {
  input_tokens: number;
  output_tokens: number;
  cache_creation_input_tokens?: number | null;
  cache_creation?: { ephemeral_5m_input_tokens?: number | null; ephemeral_1h_input_tokens?: number | null } | null;
  cache_read_input_tokens?: number | null;
}

/** An upper bound of a prompt's input tokens that `count_tokens` counted as `counted` (an estimate). */
export function inputBound(counted: number): number {
  return Math.ceil(counted * (1 + COUNT_MARGIN_RATIO)) + COUNT_MARGIN_TOKENS;
}

/**
 * The most input tokens (as `inputBound` gives them) a request may have:
 * MAX_INPUT_TOKENS, and never into a model's long-context band.
 */
export function inputLimit(price: ModelPrice): number {
  return Math.min(MAX_INPUT_TOKENS, price.longContextAbove ?? Infinity);
}

/** The rates a prompt of `promptTokens` input tokens is billed at. */
function ratesFor(price: ModelPrice, promptTokens: number): Price {
  return price.longContext && price.longContextAbove !== undefined && promptTokens > price.longContextAbove ? price.longContext : price;
}

/**
 * An upper bound on the cost of a request whose prompt `count_tokens`
 * counted as `countedInputTokens`: every input token of its bound at the
 * dearest input rate the request can incur (a 5-minute cache write), plus
 * every token `max_tokens` allows at the output rate, in the price band
 * the bound falls in.
 */
export function reservationFor(price: ModelPrice, countedInputTokens: number, maxTokens: number): number {
  const bound = inputBound(countedInputTokens);
  const rates = ratesFor(price, bound);
  return bound * rates.cacheWrite5m + maxTokens * rates.output;
}

const count = (n: number | null | undefined) => (typeof n === "number" && Number.isFinite(n) && n > 0 ? n : 0);

/**
 * The cost of a response's `usage`, in the band its whole prompt (plain,
 * written and read input) falls in. Cache writes go by TTL when the
 * breakdown is given, as 5-minute writes otherwise.
 */
export function costOf(price: ModelPrice, usage: Usage): number {
  const written = count(usage.cache_creation_input_tokens);
  const written1h = Math.min(written, count(usage.cache_creation?.ephemeral_1h_input_tokens));
  const read = count(usage.cache_read_input_tokens);
  const rates = ratesFor(price, count(usage.input_tokens) + written + read);
  return (
    count(usage.input_tokens) * rates.input +
    (written - written1h) * rates.cacheWrite5m +
    written1h * rates.cacheWrite1h +
    read * rates.cacheRead +
    count(usage.output_tokens) * rates.output
  );
}
