//! The kernel's entries for the page's scripts (in a browser only): what
//! the Python layer runs in the assistant's Web Worker. The worker loads
//! this same module (`workpiece.js` and its wasm) without starting the
//! application ([`crate::main`] starts it only where there is a window),
//! and calls:
//!
//! - `workpieceCommand(args, files)`: the `workpiece` command line
//!   ([`workpiece_make::command`], the code the native program runs) on
//!   `files` (`{path: Uint8Array}`, put in this instance's memory first);
//!   answers `{code, stdout, stderr, written}`, `written` being every
//!   file the command wrote (`{path: Uint8Array}`).
//! - `workpiecePicture(path, side, width, height, samples)`: a shaded
//!   picture of the part file at `path` (in memory), as PNG bytes.
//! - `workpieceKernelStart()`: once, first: what a panic says is kept, to
//!   be read with `workpiecePanic()` after the call it ended threw.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use workpiece_core::files;

thread_local! {
    static PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Keeps what a panic says (a panic ends the instance: the call throws,
/// and the worker starts a new one).
#[wasm_bindgen(js_name = workpieceKernelStart)]
pub fn kernel_start() {
    std::panic::set_hook(Box::new(|info| {
        let said = info.to_string();
        web_sys::console::error_1(&said.clone().into());
        PANIC.with_borrow_mut(|p| *p = Some(said));
    }));
}

/// What the last panic said, if one happened.
#[wasm_bindgen(js_name = workpiecePanic)]
pub fn kernel_panic() -> Option<String> {
    PANIC.with_borrow(Clone::clone)
}

/// Runs the command line `args` on `files`; see the module.
#[wasm_bindgen(js_name = workpieceCommand)]
pub fn command(args: Vec<String>, given: &js_sys::Object) -> Result<js_sys::Object, JsValue> {
    for entry in js_sys::Object::entries(given).iter() {
        let entry: js_sys::Array = entry.into();
        let path = entry.get(0).as_string().unwrap_or_default();
        let bytes = js_sys::Uint8Array::new(&entry.get(1)).to_vec();
        files::write(&path, &bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
    }
    let before: Vec<(String, Option<u64>)> = files::in_memory()
        .into_iter()
        .map(|p| {
            let stamp = files::stamp(&p);
            (p, stamp)
        })
        .collect();
    let stdout = RefCell::new(String::new());
    let done = workpiece_make::command(&args, &|line| {
        let mut out = stdout.borrow_mut();
        out.push_str(line);
        out.push('\n');
    });
    let written = js_sys::Object::new();
    for path in files::in_memory() {
        let was = before
            .iter()
            .find(|(p, _)| *p == path)
            .and_then(|(_, s)| *s);
        if was.is_none() || was != files::stamp(&path) {
            let bytes = files::read(&path).unwrap_or_default();
            js_sys::Reflect::set(
                &written,
                &path.as_str().into(),
                &js_sys::Uint8Array::from(bytes.as_slice()),
            )?;
        }
    }
    let answer = js_sys::Object::new();
    let set = |key: &str, value: JsValue| js_sys::Reflect::set(&answer, &key.into(), &value);
    set("code", JsValue::from(if done.is_ok() { 0 } else { 1 }))?;
    set("stdout", stdout.into_inner().into())?;
    let stderr = match &done {
        Ok(()) => String::new(),
        Err(error) => format!("error: {error:#}\n"),
    };
    set("stderr", stderr.into())?;
    set("written", written.into())?;
    Ok(answer)
}

/// A shaded picture of the part at `path` (in memory) from `side`, as
/// PNG bytes; see the module.
#[wasm_bindgen(js_name = workpiecePicture)]
pub fn picture(
    path: &str,
    side: &str,
    width: usize,
    height: usize,
    samples: usize,
) -> Result<Vec<u8>, JsValue> {
    workpiece_make::picture_of_part(path, side, [width, height], samples)
        .map_err(|e| JsValue::from_str(&format!("{e:#}")))
}
