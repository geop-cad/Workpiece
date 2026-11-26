// Builds every example's part files again in the browser's build of the kernel (the
// application's workpiece.js and workpiece_bg.wasm, as website/app/build.sh wrote them), and
// holds what they build to against what the command line built them to on the desktop, as
// examples/build_all.py keeps it in examples/built.json: the same steps, none failed, the same
// volumes, the same faces and edges, the same drawing problems. A part must build in the
// browser as it does on the desktop, so a difference is an error (the site's CI runs this
// before a deploy).
//
//   node examples/wasm_check.mjs [app dir, default website/app/dist] [example names...]
//
// Needs Node 20 or later; nothing else.
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const [appDir = join(HERE, "..", "website", "app", "dist"), ...only] = process.argv.slice(2);
// (build.sh names the two by their contents: workpiece-<hash>.js, workpiece_bg-<hash>.wasm.)
const named = (pattern) => {
  const found = readdirSync(appDir).filter((f) => pattern.test(f));
  if (found.length !== 1) throw new Error(`${appDir} has ${found.length} files like ${pattern}: build it with website/app/build.sh`);
  return join(appDir, found[0]);
};
const kernel = await import(pathToFileURL(named(/^workpiece(-[0-9a-f]+)?\.js$/)).href);
kernel.initSync({ module: readFileSync(named(/^workpiece_bg(-[0-9a-f]+)?\.wasm$/)) });
kernel.workpieceKernelStart();

const built = JSON.parse(readFileSync(join(HERE, "built.json"), "utf8"));
const listed = JSON.parse(readFileSync(join(HERE, "index.json"), "utf8")).examples.map((e) => e.name);
const names = only.length ? only : listed;

// What a `--report` says a file built to: as build_all.py's summary().
function summary(report) {
  const bodies = report.bodies.map((b) => [b.name, b.faces.length, b.edges.length]);
  for (const placed of report.placed) {
    for (const b of placed.bodies) bodies.push([`${placed.name}/${b.name}`, b.faces.length, b.edges.length]);
  }
  return {
    steps: report.steps.length,
    failed: report.failed_steps,
    volumes: Object.fromEntries(report.masses.map((m) => [m.name, m.volume])),
    bodies,
    drawing_problems: report.drawings.reduce((n, d) => n + d.inventory.problems.length, 0),
  };
}

// How `got` differs from `want`, in words; empty if it does not.
function differences(got, want) {
  const said = [];
  for (const key of ["steps", "failed", "drawing_problems"]) {
    if (got[key] !== want[key]) said.push(`${key} ${got[key]}, on the desktop ${want[key]}`);
  }
  for (const [name, volume] of Object.entries(want.volumes)) {
    const v = got.volumes[name];
    if (v === undefined || Math.abs(v - volume) > 1e-9 * Math.max(Math.abs(volume), 1)) {
      said.push(`volume of ${name} ${v}, on the desktop ${volume}`);
    }
  }
  if (JSON.stringify(got.bodies) !== JSON.stringify(want.bodies)) {
    said.push(`bodies (name, faces, edges) ${JSON.stringify(got.bodies)}, on the desktop ${JSON.stringify(want.bodies)}`);
  }
  return said;
}

const started = Date.now();
let bad = 0;
for (const name of names) {
  const want = built[name];
  if (!want) {
    console.log(`${name}: not in built.json: run python3 examples/build_all.py ${name}`);
    bad++;
    continue;
  }
  const folder = join(HERE, name);
  const files = {};
  for (const f of readdirSync(folder)) if (/\.(wpc|step|stp)$/i.test(f)) files[f] = readFileSync(join(folder, f));
  for (const [file, expected] of Object.entries(want)) {
    const t = Date.now();
    let said;
    try {
      const done = kernel.workpieceCommand(["report", file, "--report", "wasm-check.json"], files);
      const json = done.written["wasm-check.json"];
      said = json
        ? differences(summary(JSON.parse(Buffer.from(json).toString("utf8"))), expected)
        : [`no report: ${done.stderr.trim()}`];
      for (const line of done.stdout.split("\n")) if (line.startsWith("FAILED")) said.push(line);
    } catch (e) {
      // (A panic ends the instance: what it said, and nothing after it can run.)
      console.log(`${name}/${file}: the kernel broke: ${kernel.workpiecePanic() ?? e}`);
      process.exit(1);
    }
    const seconds = ((Date.now() - t) / 1000).toFixed(1);
    if (said.length) {
      bad++;
      console.log(`${name}/${file}: DIFFERS from the desktop (${seconds} s)\n  ${said.join("\n  ")}`);
    } else {
      console.log(`${name}/${file}: ${expected.steps} steps ok, as on the desktop (${seconds} s)`);
    }
  }
}
console.log(`${names.length} examples, ${bad} not as on the desktop, ${((Date.now() - started) / 1000).toFixed(1)} s`);
process.exit(bad ? 1 : 0);
