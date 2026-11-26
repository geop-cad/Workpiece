import assert from "node:assert/strict";
import { test } from "node:test";
import { onRequestGet } from "../functions/api/agent/status.ts";
import type { AgentEnv } from "../lib/agent/http.ts";

test("status is cached under one key, whatever the query string: the budget object is asked once", async () => {
  const store = new Map<string, Response>();
  (globalThis as { caches?: unknown }).caches = {
    default: {
      match: async (key: Request) => store.get(key.url)?.clone(),
      put: async (key: Request, response: Response) => void store.set(key.url, response),
    },
  };
  let asked = 0;
  const env: AgentEnv = {
    AGENT_BUDGET: { getByName: () => ({ available: async () => (asked++, true) }) } as unknown as AgentEnv["AGENT_BUDGET"],
    ANTHROPIC_API_KEY: "k",
    SESSION_SECRET: "s",
    TURNSTILE_SECRET: "t",
  };
  const handler = onRequestGet as unknown as (c: { request: Request; env: AgentEnv }) => Promise<Response>;
  for (const query of ["", "?a=1", "?a=2", "?x=" + "y".repeat(100)]) {
    const response = await handler({ request: new Request(`https://workpiece.dev/api/agent/status${query}`), env });
    assert.deepEqual(await response.json(), { available: true });
  }
  assert.equal(asked, 1);
  assert.deepEqual([...store.keys()], ["https://workpiece.dev/api/agent/status"]);
});
