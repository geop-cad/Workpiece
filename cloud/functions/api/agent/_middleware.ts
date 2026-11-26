/**
 * Every /api/agent/* request passes here first: on any host but
 * workpiece.dev (the project's *.pages.dev aliases among them) it is 404
 * before an endpoint, the budget object or the cache is touched. The
 * development hosts answer only with TURNSTILE_ALLOW_TEST_HOSTNAMES=1
 * (`wrangler pages dev`). See docs/assistant.md.
 */
import { hostAllowed, type AgentEnv } from "../../../lib/agent/http.ts";

export const onRequest: PagesFunction<AgentEnv> = async ({ request, env, next }) => {
  if (!hostAllowed(request, env)) return new Response("Not found", { status: 404, headers: { "cache-control": "no-store" } });
  return next();
};
