// A stand-in for the assistant's endpoints, to run the browser app's
// assistant panel on this computer without Cloudflare, a model or a key
// (docs/assistant.md, "Local development"):
//
//   website/app/build.sh                       # the app, into website/app/dist
//   node cloud/dev-stub.mjs [--port 8791] [--app website/app/dist]
//   open http://127.0.0.1:8791/app/?turnstile=off
//
// It serves the built app at /app/ (and the rest of website/ beside it,
// where there is any) and answers /api/agent/{session,status,turn}: any
// token opens a session; every turn request is checked with the server's
// own validation (lib/agent/validate.ts, the tools of tools.json) and
// answered by a canned model: first a run_python of a real script (a
// plate), then a view of the part, then a sentence that quotes the report
// line it was sent back. No real model is asked.
import { readFileSync, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validateTurn } from "./lib/agent/validate.ts";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const arg = (name, fallback) => {
  const at = process.argv.indexOf(name);
  return at >= 0 ? process.argv[at + 1] : fallback;
};
const PORT = Number(arg("--port", "8791"));
const APP = resolve(arg("--app", join(HERE, "../website/app/dist")));
const SITE = resolve(join(HERE, "../website"));
const TOOLS = new Set(JSON.parse(readFileSync(join(HERE, "functions/api/agent/tools.json"), "utf8")).map((t) => t.name));

/** The script the canned model writes: a plate with four holes. */
export const SCRIPT = `from workpiece import Part

p = Part(title="Plate")
s = p.sketch("XY", id="outline")
r = s.rect("a", 0, 0, 60, 40)
s.coincident(r.points[0], s.origin)
s.distance_u(r.points[0], r.points[1], 60)
s.distance_v(r.points[1], r.points[2], 40)
p.extrude(s, 8, id="plate")
report = p.build("plate.wpc")
report.show()
`;

/** The second canned answer's script ("… drawing …" in the prompt): the plate with a drawing sheet. */
export const DRAWN = SCRIPT.replace('p.extrude(s, 8, id="plate")', 'p.extrude(s, 8, id="plate")\np.drawing()').replace('"plate.wpc"', '"drawn.wpc"');

/** A script that tries every way out of the worker, and says which were blocked. */
export const NETWORK = `import js
def tried(name, f):
    try:
        f()
        print(f"{name}: OPEN")
    except Exception as e:
        print(f"{name}: blocked ({type(e).__name__})")
tried("fetch", lambda: js.fetch("https://example.com/"))
tried("XMLHttpRequest", lambda: js.XMLHttpRequest.new())
tried("WebSocket", lambda: js.WebSocket.new("wss://example.com/"))
tried("importScripts", lambda: js.importScripts("https://example.com/x.js"))
`;

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json",
  ".zip": "application/zip",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".css": "text/css",
  ".wpc": "application/json",
};

const limits = { turns_left_today: 40, turns_per_day: 40 };
let calls = 0;

/** All text of the tool results in `messages`. */
const resultsText = (messages) =>
  messages
    .flatMap((m) => (Array.isArray(m.content) ? m.content : []))
    .filter((b) => b.type === "tool_result")
    .map((b) => (typeof b.content === "string" ? b.content : b.content.map((p) => p.text ?? `[${p.type}]`).join("\n")));

/**
 * With "drawing" in the first prompt, the canned model uses every tool:
 * a script with a drawing, the sheet, the guide, the open part; then it
 * says what each answered.
 */
function drawingTurn(messages, id, message) {
  const results = resultsText(messages);
  const steps = [
    { name: "run_python", input: { script: DRAWN } },
    { name: "view", input: { sheet: "drawing1" } },
    { name: "read_guide", input: { section: "quick" } },
    { name: "get_part", input: {} },
  ];
  const next = steps[results.length];
  if (next) return message([{ type: "tool_use", id, ...next }], "tool_use");
  const [run, sheet, guide, part] = results;
  const said = [
    /ok\s+Drawing\s+drawing1/.test(run) ? "the drawing built" : "NO DRAWING",
    /\[image\]/.test(sheet) ? "the sheet came back as a picture" : `NO SHEET (${sheet})`,
    /Quick reference/.test(guide) ? "the guide was read" : `NO GUIDE (${guide.slice(0, 80)})`,
    /p\.drawing\(\)/.test(part) && /drawing drawing1/.test(part) ? "the open part has its script and its report" : `NO PART (${part.slice(0, 200)})`,
  ];
  return message([{ type: "text", text: `Done: ${said.join("; ")}.`, citations: null }], "end_turn");
}

/** The canned model's next message for `messages`. */
function turn(messages) {
  const last = messages[messages.length - 1];
  const results = Array.isArray(last.content) ? last.content.filter((b) => b.type === "tool_result") : [];
  const id = `toolu_stub_${++calls}`;
  const message = (content, stop_reason) => ({
    content,
    stop_reason,
    usage: { input_tokens: 100, output_tokens: 20, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 },
  });
  const first = messages[0].content;
  const prompt = typeof first === "string" ? first : first.map((b) => b.text ?? "").join(" ");
  if (/drawing/i.test(prompt)) return drawingTurn(messages, id, message);
  // "network": a script that tries to reach out of the worker.
  if (/network/i.test(prompt)) {
    if (results.length === 0) return message([{ type: "tool_use", id, name: "run_python", input: { script: NETWORK } }], "tool_use");
    const text = resultsText(messages).join("\n");
    const blocked = ["fetch", "XMLHttpRequest", "WebSocket", "importScripts"].every((name) => text.includes(`${name}: blocked`));
    return message([{ type: "text", text: blocked ? "Done: the network is closed to scripts." : `NO: ${text}`, citations: null }], "end_turn");
  }
  // "forever": a script that does not end (the user stops it).
  if (/forever/i.test(prompt) && results.length === 0) {
    return message([{ type: "tool_use", id, name: "run_python", input: { script: "while True:\n    pass\n" } }], "tool_use");
  }
  if (results.length === 0) {
    return message(
      [
        { type: "text", text: "I will make a 60 × 40 mm plate, 8 mm thick.", citations: null },
        { type: "tool_use", id, name: "run_python", input: { script: SCRIPT } },
      ],
      "tool_use",
    );
  }
  const text = typeof results[0].content === "string" ? results[0].content : results[0].content.map((p) => p.text ?? "").join("\n");
  const sawPicture = Array.isArray(results[0].content) && results[0].content.some((p) => p.type === "image");
  if (!sawPicture && /ok\s+Extrude/.test(text)) {
    return message([{ type: "tool_use", id, name: "view", input: { side: "iso" } }], "tool_use");
  }
  // (What every tool answered so far: the report line of the plate.)
  const all = messages
    .flatMap((m) => (Array.isArray(m.content) ? m.content : []))
    .filter((b) => b.type === "tool_result")
    .map((b) => (typeof b.content === "string" ? b.content : b.content.map((p) => p.text ?? "").join("\n")))
    .join("\n");
  const line = /^body .*$/m.exec(all)?.[0] ?? (sawPicture ? "the picture came back" : text.split("\n")[0]);
  return message([{ type: "text", text: `Done: the plate is open in the editor (${line}).`, citations: null }], "end_turn");
}

function json(res, status, body) {
  res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
  res.end(JSON.stringify(body));
}

function file(res, path) {
  if (!existsSync(path) || !statSync(path).isFile()) {
    res.writeHead(404, { "content-type": "text/plain" });
    return res.end("Not found");
  }
  res.writeHead(200, { "content-type": TYPES[extname(path)] ?? "application/octet-stream", "cache-control": "no-store" });
  res.end(readFileSync(path));
}

createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  let body = "";
  for await (const chunk of req) body += chunk;
  if (url.pathname === "/api/agent/status") return json(res, 200, { available: true });
  if (url.pathname === "/api/agent/session" && req.method === "POST") {
    const token = JSON.parse(body || "{}").turnstile;
    if (typeof token !== "string" || token === "") return json(res, 401, { error: "session", message: "No token." });
    res.setHeader("set-cookie", "workpiece_agent=stub; HttpOnly; SameSite=Strict; Path=/api/agent");
    return json(res, 200, { limits });
  }
  if (url.pathname === "/api/agent/turn" && req.method === "POST") {
    if (!/workpiece_agent=stub/.test(req.headers.cookie ?? "")) return json(res, 401, { error: "session", message: "No session." });
    let messages;
    try {
      messages = validateTurn(JSON.parse(body), Buffer.byteLength(body), TOOLS);
    } catch (e) {
      console.log(JSON.stringify({ stub: "invalid", message: e.message }));
      return json(res, 400, { error: "invalid", message: e.message });
    }
    const answer = turn(messages);
    limits.turns_left_today -= 1;
    console.log(JSON.stringify({ stub: "turn", messages: messages.length, answered: answer.content.map((b) => b.name ?? b.type).join(",") }));
    return json(res, 200, { message: answer, limits });
  }
  // The app, and the site's pages around it.
  const path = normalize(decodeURIComponent(url.pathname));
  if (path.includes("..")) return file(res, "");
  if (path === "/app" || path === "/app/") return file(res, join(APP, "index.html"));
  if (path.startsWith("/app/")) return file(res, join(APP, path.slice(5)));
  return file(res, join(SITE, path));
}).listen(PORT, "127.0.0.1", () => console.log(`stub on http://127.0.0.1:${PORT}/app/?turnstile=off (app from ${APP})`));
