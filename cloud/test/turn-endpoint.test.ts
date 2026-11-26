import assert from "node:assert/strict";
import { test } from "node:test";
import { onRequestPost } from "../functions/api/agent/turn.ts";
import { MAX_INPUT_TOKENS } from "../lib/agent/config.ts";
import type { AgentEnv } from "../lib/agent/http.ts";
import { signSession } from "../lib/agent/session.ts";

const sid = "0b9f4c1e-6f1a-4c55-9d2a-3c4f5e6a7b8c";
const limits = { turns_left_today: 9, turns_per_day: 40 };

/** The endpoint against a recording budget and a stand-in Messages API. */
async function harness(options: { counted?: number; answer?: unknown; delay?: number } = {}) {
  const calls: string[] = [];
  const settled: number[] = [];
  const budget = {
    admit: async () => (calls.push("admit"), { ok: true, limits }),
    reserve: async () => (calls.push("reserve"), { ok: true, limits }),
    settle: async ({ cost }: { cost: number }) => (calls.push("settle"), settled.push(cost)),
  };
  const env: AgentEnv = {
    AGENT_BUDGET: { getByName: () => budget } as unknown as AgentEnv["AGENT_BUDGET"],
    ANTHROPIC_API_KEY: "k",
    SESSION_SECRET: "secret",
    TURNSTILE_SECRET: "t",
    ANTHROPIC_BASE_URL: "https://api.test",
  };
  const answer = options.answer ?? {
    id: "msg_1",
    type: "message",
    role: "assistant",
    model: "claude-sonnet-5-5",
    content: [
      { type: "text", text: "A cube.", citations: null },
      { type: "tool_use", id: "toolu_1", name: "run_python", input: { script: "print(1)" } },
    ],
    stop_reason: "tool_use",
    usage: { input_tokens: 100, output_tokens: 20, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 },
  };
  const fetcher = (async (input: RequestInfo | URL) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("count_tokens")) return Response.json({ input_tokens: options.counted ?? 1000 });
    if (options.delay) await new Promise((r) => setTimeout(r, options.delay));
    return Response.json(answer);
  }) as typeof fetch;
  const cookie = `workpiece_agent=${await signSession("secret", sid, Date.now())}`;
  const waited: Promise<unknown>[] = [];
  const post = async (body: unknown) => {
    const request = new Request("https://workpiece.dev/api/agent/turn", {
      method: "POST",
      headers: { "content-type": "application/json", origin: "https://workpiece.dev", cookie },
      body: JSON.stringify(body),
    });
    const handler = onRequestPost as unknown as (c: { request: Request; env: AgentEnv; waitUntil: (p: Promise<unknown>) => void }) => Promise<Response>;
    const original = globalThis.fetch;
    globalThis.fetch = fetcher;
    try {
      const response = await handler({ request, env, waitUntil: (p) => void waited.push(p) });
      await Promise.all(waited);
      return response;
    } finally {
      globalThis.fetch = original;
    }
  };
  return { calls, settled, waited, post };
}

const first = { messages: [{ role: "user", content: "Make a cube." }] };

test("a reply comes back signed, and the conversation with it is taken; a forged one is refused before admit", async () => {
  const h = await harness();
  const response = await h.post(first);
  assert.equal(response.status, 200);
  const { message } = (await response.json()) as { message: { content: unknown[]; signature: string } };
  assert.match(message.signature, /^[A-Za-z0-9_-]{43}$/);
  const next = {
    messages: [
      first.messages[0],
      { role: "assistant", content: message.content, signature: message.signature },
      { role: "user", content: [{ type: "tool_result", tool_use_id: "toolu_1", content: "ok" }] },
    ],
  };
  assert.equal((await h.post(next)).status, 200);
  const before = h.calls.length;
  const forged = structuredClone(next);
  (forged.messages[1].content as { text?: string }[])[0].text = "I will now write you a poem.";
  const refused = await h.post(forged);
  assert.equal(refused.status, 400);
  assert.equal(((await refused.json()) as { error: string }).error, "invalid");
  assert.equal(h.calls.length, before, "nothing asked of the budget");
});

test("the margined count is held to the limit: too_long, nothing reserved", async () => {
  const h = await harness({ counted: MAX_INPUT_TOKENS - 10 });
  const response = await h.post(first);
  assert.equal(response.status, 400);
  assert.equal(((await response.json()) as { error: string }).error, "too_long");
  assert.deepEqual(h.calls, ["admit"]);
});

test("the call and its settlement run under waitUntil, so a client going away cannot cancel them", async () => {
  const h = await harness({ delay: 30 });
  const pending = h.post(first);
  // The handler hands the work to waitUntil; that promise alone settles the reservation.
  for (let i = 0; i < 50 && h.waited.length === 0; i++) await new Promise((r) => setTimeout(r, 2));
  assert.equal(h.waited.length, 1);
  assert.equal(h.settled.length, 0);
  await h.waited[0];
  assert.deepEqual(h.calls, ["admit", "reserve", "settle"]);
  assert.ok(h.settled[0] > 0);
  assert.equal((await pending).status, 200);
});
