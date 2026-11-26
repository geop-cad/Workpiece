// The browser app stays responsive while a dimension is dragged and while a part builds, in
// a headless Chrome: what the owner saw as "the web version hangs itself up while I drag
// dimensions". Each pointer move used to build the part again on the page's one thread (a
// sketch's dimension: every step after the sketch, 150–300 ms a move), so the page drew a
// few frames a second for as long as the drag went on.
//
// It serves the built app as the site does (cross-origin isolated, website/_headers) with
// the examples beside it, and
//   1. opens bearing_housing on the Annotate tab and drags the text of one of its sketch's
//      dimensions around for a few seconds, then drops it;
//   2. opens its drawing and drags the text of a dimension on the sheet the same way;
//   3. opens cast_bracket, whose first build is the longest of the examples, and watches the
//      page while it builds.
// It fails if the page is not cross-origin isolated or builds on its own thread, if the
// page's thread is held longer than LONGEST ms at once, if more than SLOW_SHARE of the drag's
// frames take longer than SLOW ms (a build per move does that to nearly all of them), if a
// pointer move is not taken within ANSWER ms, or if a dropped dimension does not end up
// where it was dropped.
//
// Why those numbers: on a CI runner (a few cores, WebGL in software) a frame of the idle app
// takes 16–50 ms. Some work is the page's alone and comes once, not per move — a drawing's
// views worked out again when the model they show changed, the meshes of a new build handed
// to WebGL: up to some 250 ms here. A build per move held the page 150–300 ms on every move,
// 1.7 s for cast_bracket's first build.
//
//   node website/app/drag.mjs [app dir, default website/app/dist] [--chrome google-chrome] [--port 8794] [--seconds 3]
//
// Run it after website/app/build.sh (CI does, in .github/workflows/site.yml); it takes
// about half a minute. Needs Chrome or Chromium; no network.
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const LONGEST = 500;
const SLOW = 100;
// The share of the time the page's thread may be held by long tasks (over 50 ms each). Frames
// are printed but not judged: a CI runner draws WebGL in software, some 130 ms a frame with
// its thread free, while a build per move held the thread nearly all of the time.
const HELD_SHARE = 0.25;
const ANSWER = 2000;

const arg = (name, fallback) => {
  const at = process.argv.indexOf(name);
  return at >= 0 ? process.argv[at + 1] : fallback;
};
const REPO = fileURLToPath(new URL("../..", import.meta.url));
const first = process.argv[2];
const APP = first && !first.startsWith("--") ? first : join(REPO, "website", "app", "dist");
const PORT = Number(arg("--port", "8794"));
const CHROME = arg("--chrome", "google-chrome");
const SECONDS = Number(arg("--seconds", "3"));
const DEBUG_PORT = PORT + 1000;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const started = Date.now();
const children = [];
const profile = mkdtempSync(join(tmpdir(), "workpiece-drag-"));
process.on("exit", () => {
  for (const child of children) child.kill("SIGKILL");
  try {
    rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
  } catch {
    // (Chrome may still be writing its profile as it ends: left to the OS.)
  }
});
function fail(message) {
  console.error(`drag: FAILED: ${message}`);
  process.exit(1);
}

// The app at /app/ with the site's headers for it, the examples at /examples/.
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".png": "image/png", ".ttf": "font/ttf", ".wpc": "application/json", ".zip": "application/zip" };
const server = createServer((req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, "http://localhost").pathname));
  let file = null;
  const headers = { "cache-control": "no-store" };
  if (path === "/app/" || path.startsWith("/app/")) {
    file = join(APP, path === "/app/" ? "index.html" : path.slice(5));
    headers["cross-origin-opener-policy"] = "same-origin";
    headers["cross-origin-embedder-policy"] = "require-corp";
  } else if (path.startsWith("/examples/")) {
    file = join(REPO, path);
  }
  if (!file || path.includes("..") || !existsSync(file) || !statSync(file).isFile()) {
    res.writeHead(404, headers);
    return res.end();
  }
  res.writeHead(200, { ...headers, "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
  res.end(readFileSync(file));
});
await new Promise((resolve) => server.listen(PORT, "127.0.0.1", resolve));

const chrome = spawn(
  CHROME,
  ["--headless=new", `--remote-debugging-port=${DEBUG_PORT}`, `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--window-size=1600,1000", "about:blank"],
  { stdio: "ignore" },
);
children.push(chrome);
let target = null;
for (let i = 0; i < 50 && !target; i++) {
  await sleep(200);
  try {
    target = (await (await fetch(`http://127.0.0.1:${DEBUG_PORT}/json/list`)).json()).find((t) => t.type === "page");
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
const logs = [];
socket.onmessage = ({ data }) => {
  const message = JSON.parse(data);
  if (message.method === "Runtime.consoleAPICalled") logs.push(message.params.args.map((a) => a.value ?? a.description ?? "").join(" "));
  if (message.method === "Runtime.exceptionThrown") logs.push(`exception: ${message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text}`);
  pending.get(message.id)?.(message);
};
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = ++nextId;
    pending.set(id, resolve);
    socket.send(JSON.stringify({ id, method, params }));
  });
const evaluate = async (expression) => (await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })).result?.result?.value;
async function until(what, expression, seconds) {
  for (let i = 0; i < seconds * 4; i++) {
    const value = await evaluate(expression).catch(() => null);
    if (value) return value;
    await sleep(250);
  }
  fail(`${what} (after ${seconds} s)${logs.length ? `; the page said:\n  ${logs.slice(-5).join("\n  ")}` : ""}`);
}
await send("Runtime.enable");
await send("Page.enable");
// Every frame the page draws, and every task that held its thread 50 ms or more.
await send("Page.addScriptToEvaluateOnNewDocument", {
  source: `window.__frames = []; window.__long = [];
    (function frame(t) { window.__frames.push(t); requestAnimationFrame(frame); })(performance.now());
    new PerformanceObserver((list) => { for (const e of list.getEntries()) window.__long.push([e.startTime, e.duration]); }).observe({ type: "longtask", buffered: true });`,
});
const crashed = `getComputedStyle(document.getElementById("crashed")).display !== "none"`;

/** How the page did since `from` (its clock): its frames, and what held its thread. */
async function since(from) {
  const frames = (await evaluate("window.__frames")).filter((t) => t >= from);
  const long = (await evaluate("window.__long")).filter(([t]) => t >= from);
  const gaps = frames.slice(1).map((t, k) => t - frames[k]);
  return {
    frames: frames.length,
    median: gaps.sort((a, b) => a - b)[Math.floor(gaps.length / 2)] ?? 0,
    slow: gaps.filter((g) => g > SLOW).length / Math.max(1, gaps.length),
    longest: Math.max(0, ...long.map(([, d]) => d)),
    held: long.reduce((sum, [, d]) => sum + d, 0) / Math.max(1, (await evaluate("performance.now()")) - from),
  };
}
const said = (s) => `${s.frames} frames, median ${s.median.toFixed(0)} ms, ${(s.slow * 100).toFixed(0)} % over ${SLOW} ms, longest hold ${s.longest.toFixed(0)} ms, held ${(s.held * 100).toFixed(0)} % of the time`;

/** Opens `query` and waits until it is built and shows its notes. */
async function open(query) {
  logs.length = 0;
  await send("Page.navigate", { url: `http://127.0.0.1:${PORT}/app/?${query}` });
  await until("the app did not start", `!!window.workpieceSeen && !document.getElementById("loading")`, 60);
  if (!(await evaluate("crossOriginIsolated"))) fail("the page is not cross-origin isolated: are the headers there?");
  const from = await evaluate("performance.now()");
  await until(`${query} did not open`, `workpieceSeen().notes.length > 0 && !workpieceSeen().building || (${crashed})`, 90);
  if (await evaluate(crashed)) fail(`the app crashed opening ${query}: ${await evaluate(`document.querySelector("#crashed pre").textContent`)}`);
  if (!logs.some((l) => /parts build on \d+ threads beside the page/.test(l))) fail(`parts build on the page's thread: ${logs.join(" | ")}`);
  // (Settled: the view put onto the part, what came of the build drawn.)
  await sleep(1500);
  return from;
}

/** Drags the text of the note `name` around and drops it; checks how the page kept up. */
async function drag(what, name) {
  const seen = await evaluate("workpieceSeen()");
  const note = seen.notes.find((n) => n.name === name);
  if (!note) fail(`${what}: no note ${name} shows (${seen.notes.map((n) => n.name).join(", ")})`);
  const mouse = (type, x, y, buttons) => send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons, clickCount: 1 });
  await mouse("mouseMoved", note.x, note.y, 0);
  await sleep(300);
  const from = await evaluate("performance.now()");
  await mouse("mousePressed", note.x, note.y, 1);
  // Round and round, 60 moves a second, each waited for as a pointer's are not: a page that
  // does not take one in time is a page that hangs.
  const moves = Math.round(SECONDS * 60);
  let slowest = 0;
  let at = [note.x, note.y];
  for (let k = 1; k <= moves; k++) {
    const turn = (k / moves) * 2 * Math.PI;
    at = [note.x + 90 * Math.sin(turn), note.y - 60 + 60 * Math.cos(turn) - 40 * (k / moves)];
    const asked = Date.now();
    const taken = await Promise.race([mouse("mouseMoved", at[0], at[1], 1).then(() => true), sleep(20000).then(() => false)]);
    if (!taken) fail(`${what}: a pointer move was not taken in 20 s: the page hangs`);
    slowest = Math.max(slowest, Date.now() - asked);
    await sleep(Math.max(0, 1000 / 60 - (Date.now() - asked)));
  }
  await mouse("mouseReleased", at[0], at[1], 0);
  const during = await since(from);
  console.log(`drag: ${what}: ${said(during)}, slowest move taken in ${slowest} ms`);
  // Dropped: it is built there beside the page, and stays there.
  const dropped = await evaluate("performance.now()");
  await until(`${what}: the build after the drop did not end`, `!workpieceSeen().building`, 60);
  await sleep(1000);
  const after = await since(dropped);
  console.log(`drag: ${what}: after the drop, ${said(after)}`);
  const now = (await evaluate("workpieceSeen()")).notes.find((n) => n.name === name);
  if (await evaluate(crashed)) fail(`${what}: the app crashed`);
  if (!now || Math.hypot(now.x - note.x, now.y - note.y) < 20) fail(`${what}: ${name} is not where it was dropped (it was at ${note.x},${note.y}; now ${now?.x},${now?.y})`);
  for (const [when, s] of [["while dragging", during], ["after the drop", after]]) {
    if (s.longest > LONGEST) fail(`${what}: ${when}, the page's thread was held ${s.longest.toFixed(0)} ms at once (at most ${LONGEST})`);
    if (s.held > HELD_SHARE) fail(`${what}: ${when}, the page's thread was held ${(s.held * 100).toFixed(0)} % of the time (at most ${HELD_SHARE * 100} %)`);
  }
  if (slowest > ANSWER) fail(`${what}: a pointer move waited ${slowest} ms to be taken (at most ${ANSWER})`);
}

// 1. A sketch's dimension, on the model.
await open("open=bearing_housing&tab=annotate");
const sketched = (await evaluate("workpieceSeen()")).notes.find((n) => /\/d\d+$/.test(n.name));
if (!sketched) fail("bearing_housing shows no dimension of a sketch");
await drag("a sketch's dimension on the model", sketched.name);
// 2. A dimension on the drawing sheet.
await open("open=bearing_housing&tab=drawing");
await drag("a dimension on the sheet", (await evaluate("workpieceSeen()")).notes[0].name);
// 3. A long build: the page draws on while it runs.
logs.length = 0;
await send("Page.navigate", { url: `http://127.0.0.1:${PORT}/app/?open=cast_bracket` });
await until("the app did not start", `!!window.workpieceSeen && !document.getElementById("loading")`, 60);
// (From the page's start: its loading, the opening, the build.)
const building = 0;
for (let i = 0; i < 240 && !logs.some((l) => l.includes("Workpiece: opened")); i++) await sleep(250);
await until("cast_bracket did not build", `!workpieceSeen().building`, 90);
const build = await since(building);
console.log(`drag: while cast_bracket builds: ${said(build)}`);
if (build.longest > LONGEST) fail(`while cast_bracket builds, the page's thread was held ${build.longest.toFixed(0)} ms at once (at most ${LONGEST})`);
if (await evaluate(crashed)) fail("the app crashed building cast_bracket");
console.log(`drag: ok (${((Date.now() - started) / 1000).toFixed(1)} s)`);
process.exit(0);
