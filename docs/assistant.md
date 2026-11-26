# The assistant in the browser app

The browser app (https://workpiece.dev/app/) has a free CAD assistant. It
works as an agent works with Workpiece on a desktop: it writes a Python
script with the authoring layer (`python/workpiece`, guide
`agent/README.md`), runs it, reads the report, looks at the part, and fixes
the script until the part is right. In the browser the script runs in
Pyodide in a Web Worker, and the layer runs the WebAssembly kernel where on
a desktop it runs the `workpiece` program; the part a script builds becomes
the part open in the editor, as one change the user can undo, with the
script kept in it.

The browser runs the agent loop and the tools: it sends the conversation to
our endpoint, gets back the model's next message, runs the tool calls, and
sends the results with the next request. The endpoint adds what the client
must not choose (our system prompt, our tools, our model, our
`max_tokens`) and accounts for every request against hard caps. It is
useless as a general LLM proxy: it takes only a conversation with our
tools, and the model is told to refuse anything but modelling.

```
browser ──POST /api/agent/session {turnstile}──▶ Pages Function ──siteverify──▶ Turnstile
        ◀── cookie workpiece_agent (HMAC, 24 h) ─┘        │
browser ──POST /api/agent/turn {messages}─────▶ Pages Function
                                                 │ 1 admit      ──▶ AgentBudget (Durable Object,
                                                 │ 2 count_tokens ─▶ Anthropic     worker workpiece-agent-budget)
                                                 │ 3 reserve    ──▶ AgentBudget
                                                 │ 4 messages   ──▶ Anthropic
                                                 │ 5 settle     ──▶ AgentBudget
        ◀── {message, limits} ───────────────────┘
```

## Where the code is

- `cloud/` — the server: `functions/api/agent/{session,turn,status}.ts` (the
  endpoints), `functions/api/agent/tools.json` (the tools), `lib/agent/`
  (configuration, validation, accounting, sessions, Turnstile, the system
  prompt), `budget-worker/` (the Durable Object), `test/` (unit tests),
  `dev-stub.mjs` (local development).
- `website/app/assistant/` — the page's side, loaded the first time the
  user opens the assistant: `agent.js` (the loop, ported from geop),
  `assistant.js` (the conversation, Turnstile, the session, the tools, the
  bridge to the app), `worker.js` (Pyodide, the layer and the kernel, in a
  Web Worker), `config.js` (the Turnstile site key), `agent.test.mjs`,
  `e2e.mjs` (end to end in headless Chrome).
- `hosts/app/src/assistant.rs` — the panel, drawn in egui (browser only:
  the native app shows nothing), and what it answers the page: the open
  part, and opening a part a script built (`Message::ReplacePart`, one
  undo step; the script is kept in `Part::script`).
- `hosts/app/src/kernel.rs` — the kernel's entries for the worker:
  `workpieceCommand(args, files)` runs the command line
  (`cad/cad-make`, crate `workpiece-make`, which the native `workpiece`
  program is a thin wrapper of) on files in memory; `workpiecePicture`.
- `python/workpiece/_backend.py` — the layer's one way to the kernel:
  natively a subprocess of the `workpiece` program, in Pyodide the worker's
  `workpieceCommand`, with the files the command reads handed over and the
  files it wrote handed back.

## The tools

| tool | input | what the browser does |
|---|---|---|
| `run_python` | `script` | runs it in Pyodide (as `python script.py` would, in the folder `/work`, which keeps what earlier scripts built); answers everything it printed (the layer's report: steps ok or failed, degrees of freedom, volumes, masses, drawing problems, lint; or a traceback), at most 20 000 characters. The part the script's last `build` wrote becomes the open part. |
| `view` | `side` (`iso`, `front`, `back`, `left`, `right`, `top`, `bottom`) or `sheet` (a Drawing step's id) | a shaded picture of the open part (800 × 600, by the kernel in the worker), or the sheet (its SVG, drawn to a canvas), as PNG, or JPEG where a PNG would pass the 300 KB limit |
| `read_guide` | `section` (optional) | a section of `agent/README.md` (the layer's `python -m workpiece.tools help <section>`); none: the list |
| `get_part` | — | the open part's name, the script kept with it, and `workpiece report` of it as it is now |

The system prompt (`cloud/lib/agent/prompt.ts`) is short: what Workpiece
is, how to work (write a script, run it, read the report, look, fix), how
the browser differs (no network, no pip, no PDF tools), and to refuse
anything but modelling. The guide is read with `read_guide`, so the cached
prefix stays small. With a prompt the page sends one line on the open part
(its name, how many steps, whether a script made it) when it changed.

The worker fetches everything it needs while it starts (Pyodide from
`/app/assistant/pyodide/`, the layer as `workpiece-python.zip`, the app's
own wasm); after that `fetch`, `XMLHttpRequest`, `WebSocket`,
`EventSource`, `importScripts` and nested workers are taken away from its
global and their prototypes, so a script can compute and answer, nothing
else. (JavaScript's dynamic `import()` cannot be taken away that way; a
`Content-Security-Policy` header on `/app/assistant/worker.js` — the
header a worker obeys is its own script's — closes it too, see "Headers"
below.) The Stop button ends the worker; the next run starts a new one
(Python is loaded again from the browser's cache in a second or two).

## Contract

- `POST /api/agent/session` `{"turnstile": "<token>"}` — verifies the
  Turnstile token with siteverify (form-encoded, 10 s timeout; HTTP ok,
  `success === true`, the action `agent_session`, the hostname
  `workpiece.dev`; a token that is not a non-empty string of at most 2048
  characters is not sent at all), then sets the cookie `workpiece_agent`
  (HttpOnly, Secure, SameSite=Strict, Path=/api/agent): a session id and
  its expiry, HMAC-signed. Answers `200 {"limits": Limits}`; any failed
  check is `401 session` and sets nothing.
- `POST /api/agent/turn` `{"messages": MessageParam[]}` — answers
  `200 {"message": {"content", "stop_reason", "usage"}, "limits": Limits}`.
  A `stop_reason` of `"refusal"` is returned like any other; the browser
  shows it and runs none of that turn's tools.
- `GET /api/agent/status` — `{"available": bool}` (cached 30 s), nothing
  else. The panel says "paused" when it is false.
- `Limits` = `{"turns_left_today": number, "turns_per_day": number}`, per
  user (the smaller of the session's and its address's remainder).
- Errors are `{"error": code, "message": text, "retry_after"?: seconds}`
  (also as a `Retry-After` header): `401 session` (no, bad or expired
  cookie, or Turnstile failed: the page shows Turnstile again, once),
  `429 rate`, `429 quota`, `503 budget` (daily or lifetime budget spent,
  the kill switch, or the endpoint not configured: "the free assistant is
  paused"), `400 invalid`, `502 upstream`. The panel shows each message as
  the server words it.

What a turn may contain (anything else is `400 invalid`): only the key
`messages`; at most 80 messages, user and assistant alternating, first and
last the user; user blocks `text` (at most 24 000 characters; the chat box
caps a typed prompt at 4000) and `tool_result` (text, or one png/jpeg
base64 image of at most 300 KB per request: the page sends every earlier
picture as a line of text); assistant blocks `text`, `tool_use` of a tool
in `tools.json`, and `thinking` / `redacted_thinking` as the model returned
them; every `tool_use` answered by a `tool_result` in the next message. The
body may be 256 KB besides the image's data. The accepted blocks are
rebuilt field by field, so nothing else the client sends reaches the API.

## Limits

All in `cloud/lib/agent/config.ts`:

| limit | value |
|---|---|
| turns per session per UTC day | 40 |
| turns per session per minute | 6 |
| turns per IP per UTC day | 80 |
| turns per IP per minute | 12 |
| new sessions per IP per UTC day | 3 |
| turns per minute, everyone | 60 |
| new sessions per UTC day, everyone | 500 |
| session lifetime | 24 h |
| input tokens per request | 100 000 |
| `max_tokens` | 8192 (thinking counts towards it) |
| model turns per prompt (the page's cap) | 25 |

The money caps are vars of the budget worker
(`cloud/budget-worker/wrangler.toml`): `AGENT_DAILY_BUDGET_USD = "20"` (per
UTC day) and `AGENT_TOTAL_BUDGET_USD = "100"` (lifetime). A missing or
unreadable value counts as 0 and closes the endpoint.

The model is `AGENT_MODEL` (`claude-sonnet-5-5`), with adaptive thinking
(summarized) at `AGENT_EFFORT` (`medium`; `low`, `medium` or `high`). A
model not in the price table `PRICES` refuses to start, since its spend
could not be accounted for. The tools and the system prompt are one cached
prefix; the growing conversation is cached at its last block.

## How the caps are enforced

Every counter lives in one Durable Object (`AgentBudget`, the instance
named `global`), which handles one call at a time; its methods do not
await, so each check-and-update is atomic. KV would not do: it is
eventually consistent and cannot hold a hard cap.

1. **Admit**: refuses if the budget is spent, the session's or the
   address's turns for the day are used up, or a minute rate is reached.
   Nothing has been asked of the API yet.
2. **Count**: `count_tokens` (free) counts the exact request. Over 100 000
   tokens is refused.
3. **Reserve**: an upper bound of the turn's cost — the counted input plus
   10 % and 500 tokens, all at the cache-write rate, plus `max_tokens` at
   the output rate. Refused unless it fits both the day's and the
   lifetime's remainder, counting every reservation still open as spent.
4. **Call** the Messages API, with the SDK's retries off (a retry could be
   billed twice against one reservation) and a 5-minute timeout.
5. **Settle** to the cost of the response's `usage`. If the API answered a
   4xx, no model ran and 0 is charged; on a 5xx, a timeout or a dropped
   connection the whole reservation is charged. A reservation never
   settled is charged in full after 15 minutes.

What could still exceed a cap: `count_tokens` is an estimate (the log line
says `over_reservation: true` if a turn cost more than it reserved); the
price table must match Anthropic's prices; a 4xx is taken as unbilled;
deleting the Durable Object forgets the lifetime spend; spend on the same
key from anywhere else is not counted. So, as a backstop: **give the
endpoint its own Anthropic Console workspace and API key, and set that
workspace's monthly spend limit** (e.g. $100).

To recharge, raise `AGENT_TOTAL_BUDGET_USD` (or the daily cap) in
`cloud/budget-worker/wrangler.toml` and push to `main`. To switch the
assistant off, set `AGENT_DISABLED = "1"` in `cloud/wrangler.toml` and push.

`GET /ledger` on the budget worker, with `Authorization: Bearer
<LEDGER_TOKEN>`, answers the lifetime spend, every day's record and the
caps (nanodollars). It answers 404 unless the secret `LEDGER_TOKEN` is set
on the worker, and the worker has no public URL (`workers_dev = false`).
The Pages Function logs one line per turn with counts and costs only,
never a message's content.

## Setup

Deployed by GitHub Actions (`.github/workflows/site.yml`) on a push to
`main`; nobody deploys with Wrangler by hand:

1. Build the site, the app included (`website/app/build.sh`, which also
   packs the layer and vendors Pyodide with `website/app/vendor.sh`), and
   `npm ci`, `npm run typecheck`, `npm test` in `cloud/`.
2. `wrangler deploy` in `cloud/budget-worker/` (first: the Pages binding
   names it).
3. `wrangler pages deploy <site> --project-name=workpiece --branch=main`
   in `cloud/` (which reads `cloud/wrangler.toml`: the Durable Object
   binding `AGENT_BUDGET`, the vars `AGENT_MODEL`, `AGENT_EFFORT`,
   `AGENT_DISABLED`).

### Secrets

On the Pages project `workpiece` (set by the owner, never in a file):

| name | what | how |
|---|---|---|
| `ANTHROPIC_API_KEY` | the key of a dedicated Console workspace | `npx wrangler pages secret put ANTHROPIC_API_KEY --project-name=workpiece` |
| `SESSION_SECRET` | random; changing it ends every session | `openssl rand -base64 32 \| npx wrangler pages secret put SESSION_SECRET --project-name=workpiece` |
| `TURNSTILE_SECRET` | the Turnstile widget's secret key | `npx wrangler pages secret put TURNSTILE_SECRET --project-name=workpiece` |

Optional, on the budget worker: `LEDGER_TOKEN` (`cd cloud/budget-worker &&
npx wrangler secret put LEDGER_TOKEN`), for `GET /ledger`.

Never set `TURNSTILE_ALLOW_TEST_HOSTNAMES` or `ANTHROPIC_BASE_URL` in
production: they are for local development.

The Turnstile widget's public site key is `TURNSTILE_SITE_KEY` in
`website/app/assistant/config.js`, the one place it is set; a page served
from this computer uses Cloudflare's always-passing test key instead. The
widget is for the hostname `workpiece.dev`, rendered with the action
`agent_session`; another hostname must be added to the widget and to
`TURNSTILE_HOSTNAMES` in `cloud/lib/agent/turnstile.ts`.

### Headers

Pyodide needs no cross-origin isolation here (a run is stopped by ending
the worker, not through a `SharedArrayBuffer`), so no `Cross-Origin-*`
headers. `.wasm` must be served as `application/wasm` and `.mjs` as
JavaScript (Cloudflare Pages does both). Recommended in `_headers`:

```
/app/assistant/worker.js
  Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval' 'unsafe-eval'; connect-src 'self'
/app/assistant/pyodide/*
  Cache-Control: public, max-age=31536000, immutable
```

(The Pyodide folder holds one pinned release; its name changes only with a
new release in `vendor.sh`, and a new release is a new set of files.)

## Local development

Without Cloudflare, a model or a key: the stub answers `/api/agent/*` with
a canned model (a `run_python` of a real script, a `view`, a last word that
quotes the report; with "drawing" in the prompt it uses every tool), checks
every request with the server's own validation, and serves the built app:

```sh
website/app/build.sh
node cloud/dev-stub.mjs              # http://127.0.0.1:8791/app/?assistant=1&turnstile=off
node website/app/assistant/e2e.mjs   # the same, end to end in headless Chrome
```

`?turnstile=off` skips the human check, on a page served from this
computer only. `?assistant=1` opens the panel at the start.

With your own key, the real functions run with `wrangler pages dev` (the
mock `scripts/mock-anthropic.mjs` stands in for the API without one):

```sh
cd cloud
(cd budget-worker && npx wrangler dev --port 8790) &
npx wrangler pages dev ../website/app/dist --port 8789 \
  --binding ANTHROPIC_API_KEY="$ANTHROPIC_API_KEY" SESSION_SECRET=local \
    TURNSTILE_SECRET=1x0000000000000000000000000000000AA TURNSTILE_ALLOW_TEST_HOSTNAMES=1
```

(served at `/`, so open `http://localhost:8789/?assistant=1`).
Cloudflare's Turnstile test keys (site key `1x00000000000000000000AA`,
secret `1x0000000000000000000000000000000AA`) pass any token, with the
hostname `example.com` and no action, which only
`TURNSTILE_ALLOW_TEST_HOSTNAMES=1` accepts.
