// The assistant's Web Worker: Python (Pyodide, vendored beside this file)
// with the `workpiece` authoring layer, and the kernel (this app's own
// WebAssembly module, `../workpiece.js`, loaded without starting the app).
// The layer's one way to the kernel is `workpieceCommand` on this worker's
// global (python/workpiece/_backend.py): the same command line the native
// program runs, on the files it is handed.
//
// Everything is fetched while starting; after that the worker can reach no
// network (fetch, XMLHttpRequest, WebSocket, EventSource, importScripts and
// nested workers are taken away), so a script the model wrote can do no
// more than compute and answer. The page stops a run by ending the worker.
//
// Messages: {id, op, ...} in, {id, ok, result} or {id, ok: false, error} out.
//   init                      load everything
//   run {script}              run a script in /work; answers {output, built, files}
//   picture {files, path, side, width, height, samples}  a PNG of a part
//   sheet {files, path, sheet}  the SVG of a drawing sheet
//   report {files, path}      what `workpiece report` prints of a part
//   guide {section}           a section of the layer's README

import init, * as kernel from "../workpiece.js";

const WORK = "/work";
let pyodide = null;
/** The part file the last `make` wrote: the part a run built last. */
let lastMade = null;
/** Set once the kernel panicked: the worker cannot go on. */
let broken = null;

/** Runs a command line on the kernel, as the layer asks (see _backend.py). */
function command(args, files) {
  if (broken) throw new Error(broken);
  const list = Array.from(args, String);
  let done;
  try {
    done = kernel.workpieceCommand(list, files);
  } catch (e) {
    broken = `the kernel stopped: ${kernel.workpiecePanic?.() ?? String(e)}`;
    throw new Error(broken);
  }
  if (list[0] === "make" && done.code === 0) {
    const at = list.indexOf("-o");
    if (at >= 0) lastMade = list[at + 1];
  }
  return done;
}

/** Takes every way out of the worker away. */
function seal() {
  const refuse = (what) =>
    function () {
      throw new Error(`${what} is not available to scripts here: there is no network`);
    };
  const names = ["fetch", "XMLHttpRequest", "WebSocket", "EventSource", "importScripts", "Worker", "SharedWorker", "BroadcastChannel"];
  const holders = [self, Object.getPrototypeOf(self)];
  for (let proto = Object.getPrototypeOf(self); proto; proto = Object.getPrototypeOf(proto)) holders.push(proto);
  for (const holder of holders) {
    for (const name of names) {
      if (Object.prototype.hasOwnProperty.call(holder, name)) {
        try {
          Object.defineProperty(holder, name, { value: refuse(name), writable: false, configurable: false });
        } catch {
          // (Not every holder lets its property be replaced.)
        }
      }
    }
  }
  if (self.navigator && "sendBeacon" in self.navigator) self.navigator.sendBeacon = refuse("sendBeacon");
}

async function start() {
  await init();
  kernel.workpieceKernelStart();
  const { loadPyodide } = await import("./pyodide/pyodide.mjs");
  pyodide = await loadPyodide({ indexURL: new URL("./pyodide/", import.meta.url).href });
  const zip = await (await fetch(new URL("./workpiece-python.zip", import.meta.url))).arrayBuffer();
  const site = pyodide.runPython("import site; site.getsitepackages()[0]");
  pyodide.unpackArchive(zip, "zip", { extractDir: site });
  self.workpieceCommand = command;
  pyodide.FS.mkdirTree(WORK);
  pyodide.runPython(`import os, workpiece, workpiece.tools; os.chdir(${JSON.stringify(WORK)})`);
  seal();
  return { python: pyodide.runPython("import sys; sys.version.split()[0]") };
}

/** The part files in /work, with when each was last written. */
function partFiles() {
  const FS = pyodide.FS;
  const found = {};
  const walk = (dir) => {
    for (const name of FS.readdir(dir)) {
      if (name === "." || name === "..") continue;
      const path = `${dir}/${name}`;
      const stat = FS.stat(path);
      if (FS.isDir(stat.mode)) {
        if (!name.endsWith(".views")) walk(path);
      } else if (name.endsWith(".wpc")) found[path] = stat.mtime.getTime?.() ?? Number(stat.mtime);
    }
  };
  walk(WORK);
  return found;
}

/** The longest a run's output is answered with: its start and its end, the middle left out. */
const LONGEST_OUTPUT = 20_000;

function clipped(text) {
  if (text.length <= LONGEST_OUTPUT) return text;
  const half = LONGEST_OUTPUT / 2;
  return `${text.slice(0, half)}\n[… ${text.length - LONGEST_OUTPUT} characters left out …]\n${text.slice(-half)}`;
}

function run({ script }) {
  const out = [];
  pyodide.setStdout({ batched: (line) => out.push(line) });
  pyodide.setStderr({ batched: (line) => out.push(line) });
  const before = partFiles();
  lastMade = null;
  const path = `${WORK}/script.py`;
  pyodide.FS.writeFile(path, script);
  const globals = pyodide.toPy({ __name__: "__main__", __file__: path });
  pyodide.globals.set("_assistant_script", script);
  pyodide.globals.set("_assistant_globals", globals);
  let failed = false;
  try {
    // As `python script.py` runs it: sys.argv[0] is the script (the layer
    // lints the script it finds there), a traceback when it raises.
    pyodide.runPython(`
import sys, traceback
sys.argv = [${JSON.stringify(path)}]
try:
    exec(compile(_assistant_script, ${JSON.stringify(path)}, "exec"), _assistant_globals)
except SystemExit as e:
    if e.code not in (None, 0):
        print(e.code if isinstance(e.code, str) else f"exit status {e.code}", file=sys.stderr)
except BaseException:
    traceback.print_exc()
    _assistant_failed = True
else:
    _assistant_failed = False
`);
    failed = pyodide.globals.get("_assistant_failed") === true;
  } finally {
    globals.destroy?.();
    pyodide.globals.delete("_assistant_script");
    pyodide.globals.delete("_assistant_globals");
  }
  // The parts written by this run, as text, by their path in /work.
  const after = partFiles();
  const files = {};
  for (const [file, stamp] of Object.entries(after)) {
    if (before[file] !== stamp) files[file.slice(WORK.length + 1)] = pyodide.FS.readFile(file, { encoding: "utf8" });
  }
  const built = lastMade && lastMade.startsWith(`${WORK}/`) && files[lastMade.slice(WORK.length + 1)] !== undefined
    ? lastMade.slice(WORK.length + 1)
    : null;
  return { output: clipped(out.join("\n")), failed, built, files };
}

/** The open part's files, put in the kernel's memory under /doc/. */
function doc(files) {
  const given = {};
  for (const [path, text] of Object.entries(files)) given[`/doc/${path}`] = new TextEncoder().encode(text);
  return given;
}

function picture({ files, path, side, width, height, samples }) {
  command(["--version"], doc(files));
  try {
    return kernel.workpiecePicture(`/doc/${path}`, side, width, height, samples);
  } catch (e) {
    if (kernel.workpiecePanic?.()) broken = `the kernel stopped: ${kernel.workpiecePanic()}`;
    throw e instanceof Error ? e : new Error(String(e));
  }
}

function sheet({ files, path, sheet }) {
  const dir = "/doc/.sheets";
  const done = command(["report", `/doc/${path}`, "--render", dir, "--no-pictures"], doc(files));
  const svg = done.written[`${dir}/${sheet}.svg`];
  if (!svg) {
    const sheets = Object.keys(done.written).filter((p) => p.endsWith(".svg")).map((p) => p.slice(dir.length + 1, -4));
    throw new Error(sheets.length ? `there is no sheet ${sheet}: the part's sheets are ${sheets.join(", ")}` : `the part has no drawing sheet${done.stderr ? `: ${done.stderr}` : ""}`);
  }
  return new TextDecoder().decode(svg);
}

function report({ files, path }) {
  const done = command(["report", `/doc/${path}`], doc(files));
  return `${done.stdout}${done.stderr}`.replaceAll("/doc/", "");
}

function guide({ section }) {
  pyodide.globals.set("_assistant_topic", section ?? null);
  try {
    return pyodide.runPython(`
from workpiece.tools import help_topic
try:
    _said = help_topic(_assistant_topic)
except SystemExit as e:
    _said = str(e)
_said`);
  } finally {
    pyodide.globals.delete("_assistant_topic");
  }
}

const OPS = { init: start, run, picture, sheet, report, guide };

self.onmessage = async ({ data }) => {
  const { id, op } = data;
  try {
    if (broken && op !== "init") throw new Error(broken);
    const result = await OPS[op](data);
    const transfer = result instanceof Uint8Array ? [result.buffer] : [];
    self.postMessage({ id, ok: true, result }, transfer);
  } catch (e) {
    self.postMessage({ id, ok: false, error: e instanceof Error ? e.message : String(e), broken: broken !== null });
  }
};
