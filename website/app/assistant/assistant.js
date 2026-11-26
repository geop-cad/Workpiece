// The assistant in the page: the conversation, the session with the
// server (proven human by Cloudflare Turnstile), the worker that runs the
// Python the model writes, and the bridge to the app, which draws the
// panel in egui (hosts/app/src/web/assistant.rs) and opens the parts.
// Loaded only once the user first opens the assistant; the loop is
// agent.js's.
//
// The bridge is `window.workpieceAssistant` (the app polls it each frame
// the panel shows):
//   version                     grows on every change of the state
//   state()                     the state, as JSON (see `snapshot`)
//   image(id)                   a picture the model looked at, as PNG bytes to show (null until ready)
//   send(text), stop(), clear()
//   onChange(fn)                fn is called after every change
//   takeRequest()               what the page asks of the app, as JSON, or null:
//                               {id, kind: "document"} — the open part — or
//                               {id, kind: "open", path, part, script, placed}
//   answer(id, json)            the app's answer to a request

import { AgentError, answer, available, describeTool, partContext, postTurn, startSession, usedUp, withPrompt } from "./agent.js";
import { LONGEST_PICTURE, LONGEST_PROMPT, LONGEST_TOOL_TEXT, SITE_KEY, SKIP_TURNSTILE } from "./config.js";

const TURNSTILE_SCRIPT = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

/** Where the API is: this site's own `/api/agent/`. */
const API = "";

const track = (event, properties) => {
  try {
    window.workpieceTrack?.(event, properties);
  } catch {
    // (Statistics never break the page.)
  }
};

// --- the state the panel shows -------------------------------------------

const state = {
  /** "starting" (Python loading), "ready", "paused" (the server takes no turns), "failed" (Python did not load). */
  status: "starting",
  statusText: "Loading Python…",
  running: false,
  verifying: false,
  /** {turns_left_today, turns_per_day} once the server told. */
  limits: null,
  /** What the conversation shows: {kind: user|assistant|tool|notice|error, text, detail?, images?, running?}. */
  entries: [],
};
const images = new Map();
let nextImage = 0;
let version = 0;
const listeners = [];

function changed() {
  version += 1;
  bridge.version = version;
  for (const f of listeners) {
    try {
      f();
    } catch {
      // (A listener that fails is the app's matter.)
    }
  }
}

const say = (entry) => {
  state.entries.push(entry);
  changed();
  return entry;
};

// --- the app: the open part, opening a part ------------------------------

const requests = [];
const waiting = new Map();
let nextRequest = 0;

/** Asks the app; answered by `answer(id, json)` on a later frame. */
function ask(request) {
  const id = ++nextRequest;
  requests.push({ id, ...request });
  changed();
  return new Promise((resolve, reject) => waiting.set(id, { resolve, reject }));
}

/** The part open in the app: {path, steps, script, files: {path: text}}, or null. */
const openDocument = () => ask({ kind: "document" });

// --- the worker ----------------------------------------------------------

let worker = null;
let ready = null;
const calls = new Map();
let nextCall = 0;

function startWorker() {
  worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  worker.onmessage = ({ data }) => {
    const call = calls.get(data.id);
    if (!call) return;
    calls.delete(data.id);
    if (data.ok) call.resolve(data.result);
    else {
      if (data.broken) restartWorker();
      call.reject(new Error(data.error));
    }
  };
  worker.onerror = (e) => {
    for (const call of calls.values()) call.reject(new Error(e.message || "the worker stopped"));
    calls.clear();
  };
  ready = call("init");
  return ready;
}

/** Ends the worker (a run that is stopped, a kernel that panicked); the next call starts another. */
function restartWorker() {
  worker?.terminate();
  for (const call of calls.values()) call.reject(new Error("stopped"));
  calls.clear();
  worker = null;
  ready = null;
}

function call(op, args = {}, transfer = []) {
  const id = ++nextCall;
  return new Promise((resolve, reject) => {
    calls.set(id, { resolve, reject });
    worker.postMessage({ id, op, ...args }, transfer);
  });
}

/** Calls the worker once it is up (starting it again if it was ended); `signal` ends the worker. */
async function work(op, args, signal) {
  if (!worker) startWorker();
  await ready;
  if (signal?.aborted) throw new Error("stopped");
  const stop = () => restartWorker();
  signal?.addEventListener("abort", stop);
  try {
    return await call(op, args);
  } finally {
    signal?.removeEventListener("abort", stop);
  }
}

// --- the tools -------------------------------------------------------------

const clip = (text) => (text.length > LONGEST_TOOL_TEXT ? `${text.slice(0, LONGEST_TOOL_TEXT)}\n[… cut off]` : text);

function base64(bytes) {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(binary);
}

/** A picture (PNG bytes, or an SVG's text) as the API takes it: PNG, or JPEG where the PNG would be too large. */
async function asPicture(bytes, type, width, height) {
  if (type === "image/png" && bytes.length * 1.34 < LONGEST_PICTURE) return { media_type: "image/png", data: base64(bytes) };
  const blob = new Blob([bytes], { type });
  const url = URL.createObjectURL(blob);
  try {
    const image = new Image();
    image.src = url;
    await image.decode();
    const w = width ?? image.naturalWidth;
    const h = height ?? image.naturalHeight;
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const g = canvas.getContext("2d");
    g.fillStyle = "#ffffff";
    g.fillRect(0, 0, w, h);
    g.drawImage(image, 0, 0, w, h);
    for (const [media, quality] of [["image/png", undefined], ["image/jpeg", 0.9], ["image/jpeg", 0.75], ["image/jpeg", 0.6]]) {
      const data = canvas.toDataURL(media, quality).split(",")[1];
      if (data.length < LONGEST_PICTURE) return { media_type: media, data };
    }
    throw new Error("the picture is too large to send");
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** A picture the model looked at, as the panel shows it: a PNG at most 640 pixels wide. */
async function thumbnail({ media_type, data }) {
  const image = new Image();
  image.src = `data:${media_type};base64,${data}`;
  await image.decode();
  const scale = Math.min(1, 640 / image.naturalWidth);
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(image.naturalWidth * scale);
  canvas.height = Math.round(image.naturalHeight * scale);
  canvas.getContext("2d").drawImage(image, 0, 0, canvas.width, canvas.height);
  const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
  return new Uint8Array(await blob.arrayBuffer());
}

/** The tools of tools.json (cloud/functions/api/agent/), run here. */
const TOOLS = {
  async run_python({ script }, signal) {
    if (typeof script !== "string" || script.trim() === "") return { text: "`script` must be the script's text.", isError: true };
    const done = await work("run", { script }, signal);
    let text = done.output || "(the script printed nothing)";
    if (done.built) {
      const opened = await ask({
        kind: "open",
        path: done.built,
        part: done.files[done.built],
        script,
        placed: Object.fromEntries(Object.entries(done.files).filter(([path]) => path !== done.built)),
      });
      text += opened.error
        ? `\n\n[${done.built} was built, but it could not be opened in the editor: ${opened.error}]`
        : `\n\n[${done.built} is now the part open in the editor.]`;
    } else if (!done.failed) {
      text += "\n\n[The script built no part: the part in the editor is as it was.]";
    }
    return { text: clip(text), isError: done.failed };
  },

  async view({ side, sheet }, signal) {
    const document = await openDocument();
    if (!document) return { text: "No part is open in the editor.", isError: true };
    if (typeof sheet === "string" && sheet !== "") {
      const svg = await work("sheet", { files: document.files, path: document.path, sheet }, signal);
      const size = /viewBox="[-\d.]+ [-\d.]+ ([\d.]+) ([\d.]+)"/.exec(svg);
      const [w, h] = size ? [Number(size[1]), Number(size[2])] : [420, 297];
      const scale = 1400 / Math.max(w, h);
      const picture = await asPicture(new TextEncoder().encode(svg), "image/svg+xml", Math.round(w * scale), Math.round(h * scale));
      return { text: `The sheet ${sheet} of ${document.path}.`, isError: false, images: [picture] };
    }
    const png = await work("picture", { files: document.files, path: document.path, side: side ?? "iso", width: 800, height: 600, samples: 48 }, signal);
    const picture = await asPicture(png, "image/png");
    return { text: `${document.path} from the ${side ?? "iso"}.`, isError: false, images: [picture] };
  },

  async read_guide({ section }, signal) {
    return { text: clip(await work("guide", { section: section ?? null }, signal)), isError: false };
  },

  async get_part(_, signal) {
    const document = await openDocument();
    if (!document) return { text: "No part is open in the editor.", isError: false };
    const report = await work("report", { files: document.files, path: document.path }, signal);
    const script = document.script
      ? `The script it was made of (the user may have changed the part by hand since):\n<script>\n${document.script}\n</script>`
      : "No script made it: the user built it by hand in the editor.";
    return { text: clip(`The part open in the editor: ${document.path}\n\n${script}\n\nA build of it as it is now:\n${report}`), isError: false };
  },
};

async function runTool(name, input, signal) {
  const tool = TOOLS[name];
  if (!tool) return { text: `There is no tool ${name}.`, isError: true };
  return tool(input ?? {}, signal);
}

// --- Turnstile and the session -------------------------------------------

let turnstileLoaded = null;

function loadTurnstile() {
  turnstileLoaded ??= new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = TURNSTILE_SCRIPT;
    script.async = true;
    script.onload = () => (window.turnstile ? resolve(window.turnstile) : reject(new Error("Turnstile did not load")));
    script.onerror = () => {
      turnstileLoaded = null;
      reject(new Error("The check that you are human could not be loaded: is challenges.cloudflare.com blocked?"));
    };
    document.head.appendChild(script);
  });
  return turnstileLoaded;
}

let widget = null;
let tokenWaiting = null;

/** A fresh Turnstile token: each is good for one session. The widget shows over the panel only when it needs the user. */
async function token() {
  if (SKIP_TURNSTILE) return "local";
  const turnstile = await loadTurnstile();
  const next = new Promise((resolve, reject) => (tokenWaiting = { resolve, reject }));
  if (widget) turnstile.reset(widget.id);
  else {
    const host = document.createElement("div");
    host.id = "assistant-turnstile";
    host.style.cssText = "position:fixed;right:24px;top:96px;z-index:20;";
    document.body.appendChild(host);
    const settle = (f) => {
      const w = tokenWaiting;
      tokenWaiting = null;
      if (w) f(w);
    };
    const id = turnstile.render(host, {
      sitekey: SITE_KEY,
      action: "agent_session",
      appearance: "interaction-only",
      callback: (t) => settle((w) => w.resolve(t)),
      "error-callback": () => settle((w) => w.reject(new Error("The check that you are human failed. Try again."))),
      "expired-callback": () => undefined,
    });
    widget = { id, host };
  }
  return next;
}

let session = null;

function ensureSession(fresh = false) {
  if (fresh) session = null;
  session ??= (async () => {
    state.verifying = true;
    changed();
    try {
      const told = await startSession(await token(), API);
      if (told) state.limits = told;
    } catch (e) {
      session = null;
      throw e;
    } finally {
      state.verifying = false;
      changed();
    }
  })();
  return session;
}

// --- asking ----------------------------------------------------------------

let messages = [];
/** The open part as the model last heard of it. */
let seen = null;
let controller = null;

function refused(e) {
  if (usedUp(e)) {
    say({ kind: "error", text: e.code === "quota" ? "You have used up today's free assistant turns. They renew at 00:00 UTC." : e.message });
    if (e.code === "budget") {
      state.status = "paused";
      state.statusText = e.message;
    }
  } else if (e instanceof AgentError && e.code === "rate") {
    const wait = e.retryAfter != null ? ` Try again in ${Math.ceil(e.retryAfter)} s.` : "";
    say({ kind: "error", text: `${e.message}${wait}` });
  } else {
    say({ kind: "error", text: e instanceof Error ? e.message : String(e) });
  }
}

async function send(prompt) {
  prompt = String(prompt ?? "").trim().slice(0, LONGEST_PROMPT);
  if (!prompt || state.running) return;
  if (state.status === "paused") return refused(new AgentError("budget", state.statusText, 503));
  say({ kind: "user", text: prompt });
  controller = new AbortController();
  state.running = true;
  changed();
  const heard = [];
  let asked = messages;
  try {
    const document = await openDocument();
    const note = partContext(document && { path: document.path, steps: document.steps, script: document.script });
    const context = note.text !== seen ? [note] : [];
    asked = withPrompt(messages, prompt, context);
    await ensureSession();
    const tools = new Map();
    const result = await answer(
      asked,
      {
        turn: async (history, signal) => {
          track("assistant_turn");
          try {
            return await postTurn(history, signal, API);
          } catch (e) {
            // The session ended: prove again to be human, once.
            if (!(e instanceof AgentError) || e.code !== "session") throw e;
            await ensureSession(true);
            return postTurn(history, signal, API);
          }
        },
        runTool,
        onMessage: (message) => {
          heard.push(message);
          messages = [...asked, ...heard];
          if (message.role !== "assistant") return;
          const said = message.content.flatMap((b) => (b.type === "text" ? [b.text] : [])).join("\n\n").trim();
          if (said && said !== "…") say({ kind: "assistant", text: said });
        },
        onToolStart: (use) => {
          const running = { run_python: "Running a script…", view: "Looking at the part…", read_guide: "Reading the guide…", get_part: "Reading the open part…" };
          tools.set(use.id, say({ kind: "tool", text: running[use.name] ?? `Running ${use.name}…`, running: true, detail: use.name === "run_python" ? use.input?.script : undefined }));
        },
        onTool: (use, result) => {
          const entry = tools.get(use.id) ?? say({ kind: "tool", text: "" });
          entry.text = describeTool(use.name, use.input, result);
          entry.running = false;
          entry.error = result.isError;
          entry.script = use.name === "run_python" ? use.input?.script : undefined;
          entry.detail = result.text;
          entry.images = (result.images ?? []).map((image) => {
            const id = `i${++nextImage}`;
            thumbnail(image).then((bytes) => {
              images.set(id, bytes);
              changed();
            }, () => undefined);
            return id;
          });
          changed();
        },
        onLimits: (limits) => {
          state.limits = limits;
          changed();
        },
      },
      controller.signal,
    );
    messages = result.messages;
    if (result.ended === "stopped") say({ kind: "notice", text: "Stopped." });
    else if (result.ended === "turns") say({ kind: "notice", text: "The answer took too many turns and was stopped. Say “continue” to go on." });
    else if (result.ended === "max_tokens") say({ kind: "notice", text: "The answer was cut off: it got too long." });
    else if (result.ended === "refusal") say({ kind: "notice", text: "The assistant declined to answer that." });
  } catch (e) {
    refused(e);
  } finally {
    state.running = false;
    controller = null;
    if (heard.length > 0) {
      const document = await openDocument().catch(() => null);
      seen = partContext(document && { path: document.path, steps: document.steps, script: document.script }).text;
    }
    changed();
  }
}

function stop() {
  controller?.abort();
}

function clear() {
  stop();
  messages = [];
  seen = null;
  state.entries = [];
  images.clear();
  changed();
}

// --- the bridge --------------------------------------------------------------

const bridge = {
  version,
  state: () => JSON.stringify(state),
  image: (id) => images.get(id) ?? null,
  send: (text) => void send(text),
  stop,
  clear,
  onChange: (f) => listeners.push(f),
  takeRequest: () => {
    const next = requests.shift();
    return next ? JSON.stringify(next) : null;
  },
  answer: (id, json) => {
    const w = waiting.get(id);
    if (!w) return;
    waiting.delete(id);
    try {
      w.resolve(JSON.parse(json));
    } catch (e) {
      w.reject(e);
    }
  },
};
window.workpieceAssistant = bridge;
track("assistant_opened");

// Python starts loading now; whether the server takes turns is asked meanwhile.
startWorker().then(
  (told) => {
    if (state.status === "starting") {
      state.status = "ready";
      state.statusText = `Python ${told.python} is ready.`;
    }
    changed();
  },
  (e) => {
    state.status = "failed";
    state.statusText = `Python could not be loaded: ${e.message}`;
    worker = null;
    ready = null;
    changed();
  },
);
available(API).then((yes) => {
  if (!yes) {
    state.status = "paused";
    state.statusText = "The free assistant is paused. It is back later; the rest of Workpiece works as it is.";
    changed();
  }
});
changed();
