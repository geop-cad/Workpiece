// The assistant end to end in a headless Chrome, against the stub endpoint
// (cloud/dev-stub.mjs): the built app loads, the assistant's panel opens,
// a prompt is sent, the canned model runs a real script in the page's
// Python on the kernel, the plate it builds becomes the open part, the
// model looks at it, and the answer ends quoting the report. A second
// conversation uses every tool: a script with a drawing, the sheet as a
// picture, the guide, and the open part with its script. Run by hand
// (or by CI, it takes about a minute) after website/app/build.sh:
//
//   node website/app/assistant/e2e.mjs [--port 8792] [--chrome google-chrome] [--shot out.png]
//
// Needs Chrome or Chromium; no network (Turnstile is skipped on this
// computer with ?turnstile=off, which only the stub takes).
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const arg = (name, fallback) => {
  const at = process.argv.indexOf(name);
  return at >= 0 ? process.argv[at + 1] : fallback;
};
const PORT = Number(arg("--port", "8792"));
const CHROME = arg("--chrome", "google-chrome");
const SHOT = arg("--shot", null);
const DEBUG_PORT = PORT + 1000;
const STUB = fileURLToPath(new URL("../../../cloud/dev-stub.mjs", import.meta.url));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const children = [];
const profile = mkdtempSync(join(tmpdir(), "workpiece-e2e-"));
function stopAll() {
  for (const child of children) child.kill("SIGKILL");
  try {
    rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
  } catch {
    // (Chrome may still be writing its profile as it ends: left to the OS.)
  }
}
process.on("exit", stopAll);

function fail(message) {
  console.error(`e2e: FAILED: ${message}`);
  process.exit(1);
}

const stub = spawn(process.execPath, [STUB, "--port", String(PORT)], { stdio: ["ignore", "pipe", "inherit"] });
children.push(stub);
stub.stdout.on("data", (d) => process.stdout.write(`[stub] ${d}`));
await sleep(800);

const chrome = spawn(
  CHROME,
  [
    "--headless=new",
    `--remote-debugging-port=${DEBUG_PORT}`,
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    "--window-size=1600,1000",
    "about:blank",
  ],
  { stdio: "ignore" },
);
children.push(chrome);

// The DevTools protocol, over the page's WebSocket.
let target = null;
for (let i = 0; i < 50 && !target; i++) {
  await sleep(200);
  try {
    const list = await (await fetch(`http://127.0.0.1:${DEBUG_PORT}/json/list`)).json();
    target = list.find((t) => t.type === "page");
  } catch {
    // (Not up yet.)
  }
}
if (!target) fail("Chrome did not start");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.onopen = resolve;
  socket.onerror = reject;
});
let nextId = 0;
const pending = new Map();
socket.onmessage = ({ data }) => {
  const message = JSON.parse(data);
  if (message.method === "Runtime.consoleAPICalled") {
    const text = message.params.args.map((a) => a.value ?? a.description ?? "").join(" ");
    console.log(`[page] ${text}`);
  }
  if (message.method === "Runtime.exceptionThrown") console.log(`[page] exception: ${message.params.exceptionDetails.text} ${message.params.exceptionDetails.exception?.description ?? ""}`);
  pending.get(message.id)?.(message);
};
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = ++nextId;
    pending.set(id, resolve);
    socket.send(JSON.stringify({ id, method, params }));
  });
const evaluate = async (expression) => {
  const answer = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (answer.result?.exceptionDetails) throw new Error(answer.result.exceptionDetails.exception?.description ?? "evaluation failed");
  return answer.result?.result?.value;
};
async function until(what, expression, seconds) {
  for (let i = 0; i < seconds * 4; i++) {
    const value = await evaluate(expression).catch(() => null);
    if (value) return value;
    await sleep(250);
  }
  fail(`${what} (after ${seconds} s)`);
}

await send("Runtime.enable");
await send("Page.enable");
const started = Date.now();
await send("Page.navigate", { url: `http://127.0.0.1:${PORT}/app/?assistant=1&turnstile=off` });
await until("the app did not start", `!document.getElementById("loading")`, 60);
console.log(`e2e: the app is up (${((Date.now() - started) / 1000).toFixed(1)} s)`);
await until("the assistant did not load", `!!window.workpieceAssistant`, 30);
await until("Python did not load", `JSON.parse(workpieceAssistant.state()).status === "ready" || JSON.parse(workpieceAssistant.state()).status === "failed"`, 90);
const status = JSON.parse(await evaluate("workpieceAssistant.state()"));
if (status.status !== "ready") fail(status.statusText);
console.log(`e2e: ${status.statusText} (${((Date.now() - started) / 1000).toFixed(1)} s)`);
await evaluate(`workpieceAssistant.send("Make a plate, 60 by 40 mm, 8 mm thick.")`);
await until("the answer did not end", `(() => { const s = JSON.parse(workpieceAssistant.state()); return !s.running && s.entries.some((e) => e.kind === "assistant" && e.text.startsWith("Done")) || s.entries.some((e) => e.kind === "error"); })()`, 120);
const state = JSON.parse(await evaluate("workpieceAssistant.state()"));
for (const e of state.entries) console.log(`e2e: [${e.kind}] ${e.text}`);
const error = state.entries.find((e) => e.kind === "error");
if (error) fail(error.text);
const run = state.entries.find((e) => e.kind === "tool" && e.text.startsWith("Ran a script"));
if (!run) fail("no script ran");
if (!/ok\s+Extrude\s+plate/.test(run.detail ?? "")) fail(`the report is not the layer's: ${run.detail}`);
if (!/plate\.wpc is now the part open in the editor/.test(run.detail ?? "")) fail(`the part did not open: ${run.detail}`);
const look = state.entries.find((e) => e.kind === "tool" && e.text.startsWith("Looked at"));
if (!look || look.error || (look.images ?? []).length !== 1) fail(`the model got no picture: ${look?.detail}`);
const done = state.entries.find((e) => e.kind === "assistant" && e.text.startsWith("Done"));
if (!/volume 19200/.test(done.text)) fail(`the report did not come back to the model: ${done.text}`);
// Every tool: a script with a drawing, its sheet as a picture, the guide, the open part.
await evaluate(`workpieceAssistant.clear(); workpieceAssistant.send("Make the plate again, with a drawing.")`);
await until("the second answer did not end", `(() => { const s = JSON.parse(workpieceAssistant.state()); return !s.running && s.entries.some((e) => e.kind === "assistant" && e.text.startsWith("Done")) || s.entries.some((e) => e.kind === "error"); })()`, 120);
const second = JSON.parse(await evaluate("workpieceAssistant.state()"));
for (const e of second.entries) console.log(`e2e: [${e.kind}] ${e.text}`);
const said = second.entries.find((e) => e.kind === "assistant" && e.text.startsWith("Done"));
if (!said || /NO /.test(said.text)) fail(said?.text ?? second.entries.find((e) => e.kind === "error")?.text);
// A script cannot reach the network from the worker.
await evaluate(`workpieceAssistant.clear(); workpieceAssistant.send("Try the network.")`);
await until("the network answer did not end", `(() => { const s = JSON.parse(workpieceAssistant.state()); return !s.running && s.entries.some((e) => e.kind === "assistant" || e.kind === "error"); })()`, 60);
const net = JSON.parse(await evaluate("workpieceAssistant.state()")).entries.find((e) => e.kind === "assistant" || e.kind === "error");
console.log(`e2e: [${net.kind}] ${net.text}`);
if (!net.text.startsWith("Done")) fail(net.text);
// A script that does not end is stopped, and the next one runs.
await evaluate(`workpieceAssistant.clear(); workpieceAssistant.send("Run forever.")`);
await sleep(3000);
await evaluate(`workpieceAssistant.stop()`);
await until("the stopped answer did not end", `(() => { const s = JSON.parse(workpieceAssistant.state()); return !s.running && s.entries.some((e) => e.text === "Stopped."); })()`, 30);
console.log("e2e: a script that ran forever was stopped");
await evaluate(`workpieceAssistant.clear(); workpieceAssistant.send("Make a plate again.")`);
await until("no answer after the stop", `(() => { const s = JSON.parse(workpieceAssistant.state()); return !s.running && s.entries.some((e) => e.kind === "assistant" && e.text.startsWith("Done")) || s.entries.some((e) => e.kind === "error"); })()`, 120);
const after = JSON.parse(await evaluate("workpieceAssistant.state()")).entries.find((e) => e.kind === "assistant" && e.text.startsWith("Done"));
if (!after) fail("the worker did not come back after a stop");
console.log("e2e: after the stop, a new worker built the plate");
await sleep(1500);
if (SHOT) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(SHOT, Buffer.from(shot.result.data, "base64"));
  console.log(`e2e: screenshot in ${SHOT}`);
}
console.log(`e2e: ok (${((Date.now() - started) / 1000).toFixed(1)} s)`);
socket.close();
process.exit(0);
