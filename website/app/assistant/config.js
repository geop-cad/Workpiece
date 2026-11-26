// The assistant's settings: the one place the Turnstile site key is set.

/**
 * The public site key of the Turnstile widget for workpiece.dev (the
 * widget's secret is the Pages project's TURNSTILE_SECRET, see
 * docs/assistant.md). A widget is bound to its hostnames: this one is for
 * workpiece.dev.
 */
export const TURNSTILE_SITE_KEY = "0x4AAAAAAGT7ThIOfcdSroJj";

/** Cloudflare's test key that always passes: what a page served from this computer uses. */
export const TURNSTILE_TEST_SITE_KEY = "1x00000000000000000000AA";

/** Whether the page is served from this computer (development). */
export const LOCAL = ["localhost", "127.0.0.1", "[::1]"].includes(globalThis.location?.hostname ?? "");

/** The site key this page uses. */
export const SITE_KEY = LOCAL ? TURNSTILE_TEST_SITE_KEY : TURNSTILE_SITE_KEY;

/**
 * On this computer only, `?turnstile=off` skips the check (for the stub
 * endpoint, cloud/dev-stub.mjs, which takes any token).
 */
export const SKIP_TURNSTILE = LOCAL && new URLSearchParams(globalThis.location?.search ?? "").get("turnstile") === "off";

/** The Pyodide the build vendors into assistant/pyodide/ (website/app/vendor.sh). */
export const PYODIDE = "./pyodide/";

/** The longest a tool's text answer is sent as (the server takes 256 KB a request besides the picture). */
export const LONGEST_TOOL_TEXT = 24_000;

/** The longest prompt the chat box takes. */
export const LONGEST_PROMPT = 4000;

/** A picture's base64 is at most this long (the server takes 300 KB). */
export const LONGEST_PICTURE = 290 * 1024;
