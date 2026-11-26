import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_TOKEN_CHARS, SITEVERIFY_TIMEOUT_MS, TURNSTILE_ACTION, TURNSTILE_HOSTNAMES, verifyTurnstile } from "../lib/agent/turnstile.ts";

/** A siteverify that answers `outcome` (with `status`), and checks what it was sent. */
function siteverify(outcome: unknown, status = 200): typeof fetch {
  return (async (url: unknown, init?: RequestInit) => {
    assert.equal(url, "https://challenges.cloudflare.com/turnstile/v0/siteverify");
    assert.equal(init?.method, "POST");
    assert.equal((init?.headers as Record<string, string>)["content-type"], "application/x-www-form-urlencoded");
    assert.ok(init?.signal instanceof AbortSignal, "a timeout is set");
    const form = new URLSearchParams(init?.body as string);
    assert.equal(form.get("secret"), "secret");
    assert.equal(form.get("response"), "token");
    assert.equal(form.get("remoteip"), "1.2.3.4");
    return Response.json(outcome, { status });
  }) as typeof fetch;
}

const check = (outcome: unknown, allowTest = false, status = 200) =>
  verifyTurnstile("secret", "token", "1.2.3.4", allowTest, siteverify(outcome, status));
const good = { success: true, action: TURNSTILE_ACTION, hostname: "workpiece.dev", "error-codes": [] };

test("success, our action and our hostname pass", async () => {
  assert.deepEqual(await check(good), { passed: true });
});

test("the production hostnames are exactly workpiece.dev", () => {
  assert.deepEqual([...TURNSTILE_HOSTNAMES], ["workpiece.dev"]);
});

test("a token that is not a string, empty or too long is refused without asking siteverify", async () => {
  const never = (async () => assert.fail("siteverify must not be asked")) as typeof fetch;
  for (const token of [undefined, null, 42, {}, "", "x".repeat(MAX_TOKEN_CHARS + 1)]) {
    assert.deepEqual(await verifyTurnstile("secret", token, "1.2.3.4", false, never), { passed: false, reason: "token" });
  }
});

test("success with another action is refused", async () => {
  assert.deepEqual(await check({ ...good, action: "login" }), { passed: false, reason: "action" });
  assert.deepEqual(await check({ ...good, action: undefined }), { passed: false, reason: "action" });
});

test("success on another hostname is refused", async () => {
  for (const hostname of ["evil.example", "app.geop-cad.dev", "www.workpiece.dev", "localhost", "127.0.0.1", "example.com", undefined]) {
    assert.deepEqual(await check({ ...good, hostname }), { passed: false, reason: "hostname" });
  }
});

test("no success is refused, with only the error codes kept", async () => {
  assert.deepEqual(await check({ success: false, "error-codes": ["timeout-or-duplicate"] }), { passed: false, reason: "unsuccessful: timeout-or-duplicate" });
  assert.deepEqual(await check({ success: "true", action: TURNSTILE_ACTION, hostname: "workpiece.dev" }), { passed: false, reason: "unsuccessful: " });
});

test("an answer that is not HTTP ok is refused, whatever it says", async () => {
  assert.deepEqual(await check(good, false, 500), { passed: false, reason: "siteverify answered 500" });
});

test("siteverify unreachable, not JSON, or past its timeout is refused", async () => {
  const down = (async () => { throw new TypeError("network"); }) as typeof fetch;
  assert.deepEqual(await verifyTurnstile("secret", "token", "1.2.3.4", false, down), { passed: false, reason: "siteverify unreachable" });
  const garbage = (async () => new Response("<html>")) as typeof fetch;
  assert.deepEqual(await verifyTurnstile("secret", "token", "1.2.3.4", false, garbage), { passed: false, reason: "siteverify unreachable" });
  assert.equal(SITEVERIFY_TIMEOUT_MS, 10_000);
  // A siteverify that never answers: the request's signal ends it.
  const hangs = ((_: unknown, init?: RequestInit) =>
    new Promise((_resolve, reject) => init?.signal?.addEventListener("abort", () => reject(init.signal?.reason)))) as typeof fetch;
  const original = AbortSignal.timeout;
  AbortSignal.timeout = () => original.call(AbortSignal, 10);
  try {
    assert.deepEqual(await verifyTurnstile("secret", "token", "1.2.3.4", false, hangs), { passed: false, reason: "siteverify unreachable" });
  } finally {
    AbortSignal.timeout = original;
  }
});

test("test hostnames only with the dev flag, and the production list has no local host", async () => {
  const testKey = { success: true, hostname: "example.com", "error-codes": [], metadata: { result_with_testing_key: true } };
  assert.equal((await check(testKey)).passed, false);
  assert.equal((await check(testKey, true)).passed, true);
  for (const local of ["localhost", "127.0.0.1"]) assert.ok(!TURNSTILE_HOSTNAMES.includes(local));
});
