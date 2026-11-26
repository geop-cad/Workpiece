import assert from "node:assert/strict";
import { test } from "node:test";
import { onRequestPost } from "../functions/api/agent/session.ts";
import type { AgentEnv } from "../lib/agent/http.ts";

/** The endpoint with a budget that fails the test if a session is opened, and siteverify answering `outcome`. */
async function post(body: unknown, outcome: unknown | Error, status = 200) {
  const env: AgentEnv = {
    AGENT_BUDGET: { getByName: () => ({ openSession: () => assert.fail("no session may be opened") }) } as unknown as AgentEnv["AGENT_BUDGET"],
    ANTHROPIC_API_KEY: "k",
    SESSION_SECRET: "s",
    TURNSTILE_SECRET: "t",
  };
  const original = globalThis.fetch;
  globalThis.fetch = (async () => {
    if (outcome instanceof Error) throw outcome;
    return Response.json(outcome, { status });
  }) as typeof fetch;
  try {
    const request = new Request("https://workpiece.dev/api/agent/session", {
      method: "POST",
      headers: { "content-type": "application/json", origin: "https://workpiece.dev" },
      body: JSON.stringify(body),
    });
    const handler = onRequestPost as unknown as (c: { request: Request; env: AgentEnv }) => Promise<Response>;
    return await handler({ request, env });
  } finally {
    globalThis.fetch = original;
  }
}

const good = { success: true, action: "agent_session", hostname: "workpiece.dev" };

test("every failed check is 401 session, and no cookie is set", async () => {
  const cases: [unknown, unknown, number?][] = [
    [{ turnstile: 42 }, good],
    [{ turnstile: "" }, good],
    [{ turnstile: "x".repeat(2049) }, good],
    [{}, good],
    [{ turnstile: "t" }, good, 500],
    [{ turnstile: "t" }, { ...good, success: false }],
    [{ turnstile: "t" }, { ...good, action: "other" }],
    [{ turnstile: "t" }, { ...good, hostname: "localhost" }],
    [{ turnstile: "t" }, new TypeError("network")],
  ];
  for (const [body, outcome, status] of cases) {
    const response = await post(body, outcome, status);
    assert.equal(response.status, 401, JSON.stringify(body));
    assert.equal((await response.json() as { error: string }).error, "session");
    assert.equal(response.headers.get("set-cookie"), null);
  }
});
