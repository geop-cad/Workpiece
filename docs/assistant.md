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
tools, every assistant message in it must carry the signature the endpoint
gave it in this session (so the client cannot write the model's side), and
the model is told to refuse anything but modelling.

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
  endpoints), `functions/api/agent/_middleware.ts` (the host guard in front
  of all three), `functions/api/agent/tools.json` (the tools), `lib/agent/`
  (configuration, validation, message signatures, accounting, sessions,
  Turnstile, the system prompt), `budget-worker/` (the Durable Object), `test/` (unit tests),
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
own wasm); after that every API a worker can make a request with is taken
away from its global, its navigator and their prototypes (`NETWORK_APIS`
in `worker.js`: `fetch`, `XMLHttpRequest`, `WebSocket`, `WebSocketStream`,
`EventSource`, `WebTransport`, `importScripts`, `Worker`, `SharedWorker`,
`BroadcastChannel`, `caches` / `CacheStorage` / `Cache`, `FontFace` /
`fonts`, `Notification`, `RTCPeerConnection`, `fetchLater`, and
`navigator.serviceWorker` / `navigator.sendBeacon`; the e2e test reads the
same list and checks each), so a script can compute and answer, nothing
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
  `200 {"message": {"content", "stop_reason", "usage", "signature"},
  "limits": Limits}`. `content` is the model's answer as the conversation
  keeps it (a tool use only when `stop_reason` is `tool_use`, no empty
  text, `[{"type": "text", "text": "…"}]` for an empty answer); the client
  appends it unchanged as `{"role": "assistant", "content", "signature"}`.
  `signature` is HMAC-SHA256 under a key derived from `SESSION_SECRET`
  (`HMAC(SESSION_SECRET, "workpiece assistant message signature v1")`)
  over the session id, a newline and the content as the server rebuilds it,
  serialised with sorted keys (`cloud/lib/agent/signature.ts`); it is null
  only for content a turn could not carry back. A `stop_reason` of
  `"refusal"` is returned like any other; the browser shows it and runs
  none of that turn's tools.
- `GET /api/agent/status` — `{"available": bool}` (cached 30 s under one
  key, whatever the query string), nothing else. The panel says "paused"
  when it is false.
- On any host but `workpiece.dev` (the project's `*.pages.dev` aliases
  among them) every `/api/agent/*` request is `404` before an endpoint, the
  cache or the budget object is reached; `localhost`, `127.0.0.1` and
  `[::1]` answer only with `TURNSTILE_ALLOW_TEST_HOSTNAMES=1`.
- `Limits` = `{"turns_left_today": number, "turns_per_day": number}`, per
  user (the smaller of the session's and its address's remainder; 0 once
  the session's or the address's money for the day is spent).
- Errors are `{"error": code, "message": text, "retry_after"?: seconds}`
  (also as a `Retry-After` header): `401 session` (no, bad or expired
  cookie, or Turnstile failed: the page shows Turnstile again, once),
  `429 rate`, `429 quota` (the turns or the money of the session or of the
  address for the day), `503 budget` (daily or lifetime budget spent, the
  kill switch, or the endpoint not configured: "the free assistant is
  paused"), `400 invalid`, `400 too_long` (more input tokens than a request
  may have: the page compacts the conversation and asks once more),
  `502 upstream`. The panel shows each message as the server words it.

What a turn may contain (anything else is `400 invalid`): only the key
`messages`; at most 80 messages, user and assistant alternating, first and
last the user; user blocks `text` (not empty, at most 24 000 characters;
the chat box caps a typed prompt at 4000) and `tool_result` (text, as a
string or text parts, each not empty and at most 24 000 characters, and
one png/jpeg base64 image of at most 300 KB per request: the page sends
every earlier picture as a line of text); assistant messages
`{"role", "content", "signature"}`, every one with a valid signature for
this session (checked before anything is counted), of blocks `text` (not
empty), `tool_use` of a tool in `tools.json`, and `thinking` /
`redacted_thinking` as the model returned them; tool use ids
`^[A-Za-z0-9_-]{1,64}$`; every `tool_use` answered by a `tool_result` in
the next message. The body may be 128 KB besides the image's data, read as
a stream and given up past its cap whatever its `Content-Length` says. The
accepted blocks are rebuilt field by field, so nothing else the client
sends reaches the API.

The page keeps every request under these limits: a tool's text is clipped
to 24 000 characters (`toolText` in `agent.js`), and once the conversation
passes 96 KB (`COMPACT_AT_BYTES`), when the server answers `too_long`, or
after the session was renewed (the old messages are signed for the old
session), it is compacted into one user message: the user's earlier
messages, what the assistant said last, the last script and what it
printed, and the last tools' answers, then the user's latest words as they
were (`compact`; no assistant message or thinking is carried over, which is
the shape the API's preserved-thinking check allows).

## Limits

All in `cloud/lib/agent/config.ts`:

| limit | value |
|---|---|
| turns per session per UTC day | 40 |
| turns per session per minute | 6 |
| money per session per UTC day | 0.75 USD |
| turns per IP per UTC day | 80 |
| turns per IP per minute | 12 |
| money per IP per UTC day | 1.50 USD |
| new sessions per IP per UTC day | 3 |
| turns per minute, everyone (reserved turns only) | 60 |
| new sessions per UTC day, everyone | 20 000 |
| session lifetime | 24 h |
| input tokens per request (the count plus its margin) | 40 000 |
| request body besides the image | 128 KB |
| `max_tokens` | 8192 (thinking counts towards it) |
| model turns per prompt (the page's cap) | 25 |

"IP" is the address in `CF-Connecting-IP`: an IPv4 address whole, an IPv6
address by its /64 (`clientIp` in `lib/agent/http.ts`). Turns per day and
per minute count from `admit` on, so a request refused later (by
`count_tokens`, say) still costs the caller its own turn and minute slot;
only a turn that reserved counts towards everyone's minute window, so
refused requests cannot fill it. Money per session and per IP: a turn's
reservation must fit the remainder (open reservations counted), and it is
settled to the real cost. A typical turn costs 1–4 cents with the cache, so
0.75 USD is some 20–40 turns of a session; the worst turn reserves about
0.19 USD (Sonnet 5.5: 40 000 tokens plus margin at the cache-write rate and
8192 output tokens). One address can take at most 1.50 USD, 7.5 % of the
20 USD daily cap.

What remains possible: someone with many IPv4 addresses or /64s and a
Turnstile-solving service can open up to 3 sessions per address and spend
1.50 USD per address, so about 14 addresses spend the day's 20 USD and
pause the assistant for everyone until 00:00 UTC (the lifetime cap and the
Console workspace's spend limit still hold). Locking everyone out of new
sessions takes 20 000 solved challenges from 6 667 addresses in a day.

The money caps are vars of the budget worker
(`cloud/budget-worker/wrangler.toml`): `AGENT_DAILY_BUDGET_USD = "20"` (per
UTC day) and `AGENT_TOTAL_BUDGET_USD = "100"` (lifetime). A missing or
unreadable value counts as 0 and closes the endpoint.

The model is `AGENT_MODEL` (`claude-sonnet-5-5`), with adaptive thinking
(summarized) at `AGENT_EFFORT` (`medium`; `low`, `medium` or `high`). A
model not in the price table `PRICES` refuses to start, since its spend
could not be accounted for. The table holds Anthropic's list prices
(checked 2026-11-26): Opus 5.5 $4 / $20 per million tokens, cache reads
$0.20; Sonnet 5.5 $2 / $10, cache reads $0.10; Haiku 5.5 $0.10 / $0.50 up
to 100K prompt tokens and $0.50 / $2.50 above (priced in that band, and a
request never reaches it: the input limit is the smaller of 40 000 and a
model's band). Cache writes are 1.25× (5 minutes) or 2× (1 hour) input. The tools and the system prompt are one cached
prefix; the growing conversation is cached at its last block.

## How the caps are enforced

Every counter lives in one Durable Object (`AgentBudget`, the instance
named `global`), which handles one call at a time; its methods do not
await, so each check-and-update is atomic. KV would not do: it is
eventually consistent and cannot hold a hard cap.

0. **Verify**: the cookie, the body (streamed, capped), its shape, and the
   signature of every assistant message. Nothing is counted yet.
1. **Admit**: refuses if the budget is spent, the session's or the
   address's turns or money for the day are used up, or a minute rate is
   reached. Counts the turn in the caller's own minute windows and day.
   Nothing has been asked of the API yet.
2. **Count**: `count_tokens` (free) counts the exact request. The count
   plus 10 % and 500 tokens over 40 000 is refused (`too_long`).
3. **Reserve**: an upper bound of the turn's cost — the counted input plus
   10 % and 500 tokens, all at the cache-write rate, plus `max_tokens` at
   the output rate. Refused unless it fits the day's and the lifetime's
   remainder and the session's and the address's money for the day,
   counting every reservation still open as spent, and everyone's minute
   rate allows it (where it is counted).
4. **Call** the Messages API, with the SDK's retries off (a retry could be
   billed twice against one reservation) and a 5-minute timeout. The call
   and step 5 run in a promise handed to `waitUntil`, which the response
   also awaits, so a client that goes away does not cancel them.
5. **Settle** to the cost of the response's `usage`, charged to the day,
   the session and the address. If the API answered a
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
