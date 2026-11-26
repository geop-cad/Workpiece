import assert from "node:assert/strict";
import { test } from "node:test";
import { onRequest } from "../functions/api/agent/_middleware.ts";
import { clientIp, hostAllowed, readBody, type AgentEnv } from "../lib/agent/http.ts";

const from = (ip: string | null) => new Request("https://workpiece.dev/", { headers: ip ? { "CF-Connecting-IP": ip } : {} });

test("IPv4 counts whole, IPv6 by its /64", () => {
  assert.equal(clientIp(from("203.0.113.7")), "203.0.113.7");
  assert.equal(clientIp(from("2001:db8:1:2:aaaa:bbbb:cccc:dddd")), "2001:db8:1:2::/64");
  assert.equal(clientIp(from("2001:0db8:0001:0002:0:0:0:1")), "2001:db8:1:2::/64");
  assert.equal(clientIp(from("2001:db8:1:2::ffff")), "2001:db8:1:2::/64");
  assert.equal(clientIp(from("2001:DB8::1")), "2001:db8:0:0::/64");
  assert.equal(clientIp(from("::1")), "0:0:0:0::/64");
  assert.equal(clientIp(from("::ffff:198.51.100.4")), "198.51.100.4");
  assert.equal(clientIp(from("2001:db8:1:2:3:4:1.2.3.4")), "2001:db8:1:2::/64");
  assert.equal(clientIp(from(null)), "unknown");
  // Not an address: kept apart from every real one.
  assert.match(clientIp(from("1:2:3")), /^\?/);
  assert.match(clientIp(from("1::2::3")), /^\?/);
});

test("the endpoints answer on workpiece.dev only, development hosts under the test flag", async () => {
  const at = (url: string) => new Request(url);
  const prod: AgentEnv = {};
  const dev: AgentEnv = { TURNSTILE_ALLOW_TEST_HOSTNAMES: "1" };
  assert.ok(hostAllowed(at("https://workpiece.dev/api/agent/status"), prod));
  for (const url of ["https://workpiece.pages.dev/api/agent/status", "https://abc123.workpiece.pages.dev/api/agent/turn", "http://localhost:8789/api/agent/turn"]) {
    assert.equal(hostAllowed(at(url), prod), false, url);
  }
  assert.ok(hostAllowed(at("http://localhost:8789/api/agent/turn"), dev));
  assert.equal(hostAllowed(at("https://workpiece.pages.dev/api/agent/turn"), dev), false);
  // The middleware: 404 before anything else runs.
  const handler = onRequest as unknown as (c: { request: Request; env: AgentEnv; next: () => Promise<Response> }) => Promise<Response>;
  let reached = 0;
  const next = async () => (reached++, new Response("ok"));
  assert.equal((await handler({ request: at("https://workpiece.pages.dev/api/agent/status"), env: prod, next })).status, 404);
  assert.equal(reached, 0);
  assert.equal((await handler({ request: at("https://workpiece.dev/api/agent/status"), env: prod, next })).status, 200);
  assert.equal(reached, 1);
});

test("a body is read up to its cap, whatever its content-length says", async () => {
  const streamOf = (bytes: number, chunk = 1000) =>
    new ReadableStream<Uint8Array>({
      start(controller) {
        for (let sent = 0; sent < bytes; sent += chunk) controller.enqueue(new Uint8Array(Math.min(chunk, bytes - sent)));
        controller.close();
      },
    });
  const post = (bytes: number, length?: string) =>
    new Request("https://workpiece.dev/", { method: "POST", body: streamOf(bytes), headers: length ? { "content-length": length } : {}, duplex: "half" } as RequestInit);
  assert.equal((await readBody(post(5000), 5000))?.length, 5000);
  assert.equal(await readBody(post(5001), 5000), null);
  // A content-length that lies low does not let more through.
  assert.equal(await readBody(post(50_000, "10"), 5000), null);
  assert.equal(await readBody(post(10, "999999"), 5000), null);
});
