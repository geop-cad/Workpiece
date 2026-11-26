// One of the build engine's threads (cad/cad-part/src/pool.rs), as the page starts it
// (hosts/app/src/web.rs, spawn): the program, started on the page's own memory, runs the
// work it is handed — waiting for parts to build, building them — until the page goes.
// The page sends the compiled module, its memory and the work. If the work breaks (a
// panic ends the program in a browser), the page is told, and says so.
//
// build.sh writes the program's name by its contents in place of ./workpiece.js.
import { initSync, workpieceThread } from "./workpiece.js";

self.onmessage = ({ data: [module, memory, work] }) => {
  // (A stack as large as the desktop's threads have: some steps recurse deep.)
  initSync({ module, memory, thread_stack_size: 8 << 20 });
  try {
    workpieceThread(work);
  } catch (e) {
    self.postMessage(`${e}`);
  }
  close();
};
