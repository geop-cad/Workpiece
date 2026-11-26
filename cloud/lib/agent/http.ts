/** What the agent endpoints share: their environment, its configuration, and the JSON error contract. */
import type { AgentBudget } from "../../budget-worker/src/index.ts";
import { DEFAULT_EFFORT, DEFAULT_MODEL, EFFORTS, PRICES, type Effort, type ModelPrice } from "./config.ts";
import type { Refusal } from "./ledger.ts";

/** The Pages project's bindings, secrets and vars (see docs/assistant.md). */
export interface AgentEnv {
  /** Durable Object binding to the class `AgentBudget` of the worker `workpiece-agent-budget`. */
  AGENT_BUDGET?: DurableObjectNamespace<AgentBudget>;
  ANTHROPIC_API_KEY?: string;
  SESSION_SECRET?: string;
  TURNSTILE_SECRET?: string;
  AGENT_MODEL?: string;
  AGENT_EFFORT?: string;
  /** "1" turns the assistant off. */
  AGENT_DISABLED?: string;
  /**
   * "1" accepts Turnstile test keys' hostnames, and serves /api/agent/* on
   * development hosts (DEV_HOSTS): for `wrangler pages dev` only, never in
   * production.
   */
  TURNSTILE_ALLOW_TEST_HOSTNAMES?: string;
  /** For a local smoke test against a mock only; never set in production. */
  ANTHROPIC_BASE_URL?: string;
}

export type ErrorCode = "session" | "rate" | "quota" | "budget" | "invalid" | "too_long" | "upstream";

export function errorResponse(status: number, error: ErrorCode, message: string, retryAfter?: number): Response {
  const headers: Record<string, string> = { "cache-control": "no-store" };
  if (retryAfter !== undefined) headers["retry-after"] = String(retryAfter);
  return Response.json({ error, message, ...(retryAfter !== undefined ? { retry_after: retryAfter } : {}) }, { status, headers });
}

export function refusalResponse(refusal: Refusal): Response {
  return errorResponse(refusal.error === "budget" ? 503 : 429, refusal.error, refusal.message, refusal.retry_after);
}

export const unavailable = () => errorResponse(503, "budget", "The free assistant is paused.");

export interface Setup {
  budget: DurableObjectStub<AgentBudget>;
  apiKey: string;
  sessionSecret: string;
  turnstileSecret: string;
  model: string;
  price: ModelPrice;
  effort: Effort;
  baseURL?: string;
}

/**
 * The configuration, or null when the assistant must not run: switched
 * off, a secret or the budget binding missing, or a model without a price
 * (whose spend could not be accounted for).
 */
export function setupOf(env: AgentEnv): Setup | null {
  if (env.AGENT_DISABLED === "1") return null;
  const model = env.AGENT_MODEL || DEFAULT_MODEL;
  const price = PRICES[model];
  const effort = (env.AGENT_EFFORT || DEFAULT_EFFORT) as Effort;
  if (!price || !EFFORTS.includes(effort)) {
    console.log(JSON.stringify({ event: "agent_misconfigured", model, effort }));
    return null;
  }
  if (!env.AGENT_BUDGET || !env.ANTHROPIC_API_KEY || !env.SESSION_SECRET || !env.TURNSTILE_SECRET) {
    console.log(JSON.stringify({ event: "agent_misconfigured", missing: "binding or secret" }));
    return null;
  }
  return {
    budget: env.AGENT_BUDGET.getByName("global"),
    apiKey: env.ANTHROPIC_API_KEY,
    sessionSecret: env.SESSION_SECRET,
    turnstileSecret: env.TURNSTILE_SECRET,
    model,
    price,
    effort,
    baseURL: env.ANTHROPIC_BASE_URL || undefined,
  };
}

/** The 8 groups of an IPv6 address, or null if `text` is not one. An embedded IPv4 tail counts as two groups. */
function ipv6Groups(text: string): number[] | null {
  let address = text.toLowerCase().split("%")[0];
  const v4 = /(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(address);
  if (v4) {
    const bytes = v4.slice(1).map(Number);
    if (bytes.some((b) => b > 255)) return null;
    address = `${address.slice(0, v4.index)}${((bytes[0] << 8) | bytes[1]).toString(16)}:${((bytes[2] << 8) | bytes[3]).toString(16)}`;
  }
  const halves = address.split("::");
  if (halves.length > 2) return null;
  const parse = (part: string) => (part === "" ? [] : part.split(":"));
  const head = parse(halves[0]);
  const tail = halves.length === 2 ? parse(halves[1]) : [];
  const missing = 8 - head.length - tail.length;
  if (halves.length === 2 ? missing < 1 : missing !== 0) return null;
  const groups = [...head, ...Array(halves.length === 2 ? missing : 0).fill("0"), ...tail];
  if (!groups.every((g) => /^[0-9a-f]{1,4}$/.test(g))) return null;
  return groups.map((g) => parseInt(g, 16));
}

/**
 * Who a request is counted against: CF-Connecting-IP, an IPv4 address
 * whole, an IPv6 address by its /64 (`2001:db8:1:2::/64`: one subscriber
 * gets a /64 or more, so the addresses inside one are one caller). An
 * IPv4-mapped IPv6 address counts as its IPv4 address.
 */
export function clientIp(request: Request): string {
  const header = (request.headers.get("CF-Connecting-IP") ?? "").trim();
  if (!header.includes(":")) return header || "unknown";
  const groups = ipv6Groups(header);
  if (!groups) return `?${header.slice(0, 64)}`;
  if (groups.slice(0, 5).every((g) => g === 0) && groups[5] === 0xffff) {
    return `${groups[6] >> 8}.${groups[6] & 255}.${groups[7] >> 8}.${groups[7] & 255}`;
  }
  return `${groups.slice(0, 4).map((g) => g.toString(16)).join(":")}::/64`;
}

/** The host the assistant's endpoints answer on. */
export const PRODUCTION_HOST = "workpiece.dev";
/** Hosts they also answer on with `TURNSTILE_ALLOW_TEST_HOSTNAMES=1` (`wrangler pages dev`). */
export const DEV_HOSTS: readonly string[] = ["localhost", "127.0.0.1", "[::1]"];

/**
 * Whether /api/agent/* answers on this request's host: workpiece.dev only
 * (not the project's *.pages.dev aliases, which would also reach the
 * budget object), and the development hosts under the test flag.
 */
export function hostAllowed(request: Request, env: AgentEnv): boolean {
  const host = new URL(request.url).hostname;
  return host === PRODUCTION_HOST || (env.TURNSTILE_ALLOW_TEST_HOSTNAMES === "1" && DEV_HOSTS.includes(host));
}

/**
 * The body, read as a stream and given up as soon as it passes `limit`
 * bytes (whatever its content-length says): null when it is longer.
 */
export async function readBody(request: Request, limit: number): Promise<Uint8Array | null> {
  if (Number(request.headers.get("content-length") ?? "0") > limit) return null;
  if (!request.body) return new Uint8Array(0);
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > limit) {
      await reader.cancel().catch(() => undefined);
      return null;
    }
    chunks.push(value);
  }
  const body = new Uint8Array(length);
  let at = 0;
  for (const chunk of chunks) {
    body.set(chunk, at);
    at += chunk.byteLength;
  }
  return body;
}

/** Refuses a cross-site POST: the cookie is SameSite=Strict already, this also holds for a browser that ignores it. */
export function sameOrigin(request: Request): boolean {
  const origin = request.headers.get("origin");
  return origin === null || origin === new URL(request.url).origin;
}
