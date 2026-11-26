# cloud/: what runs on Cloudflare

The server side of https://workpiece.dev/: the Pages Functions of the Pages
project `workpiece` (the assistant's `/api/agent/*`), and the budget worker
they bind. Both are deployed only by GitHub Actions
(`.github/workflows/site.yml`): first `budget-worker/` (`wrangler deploy`),
then the Pages project from this directory (`wrangler pages deploy <site>
--project-name=workpiece --branch=main`: Wrangler finds `functions/`
relative to the directory it runs in, and reads `wrangler.toml` here, which
holds every binding and var of the project; the secrets are set on the
project).

```sh
npm ci
npm run typecheck
npm test           # node >= 23.6 (runs the .ts tests directly); also the page's loop tests
npm run stub       # the endpoints without Cloudflare, for the browser app (see below)
```

- `functions/api/agent/` — the assistant: `_middleware.ts` (404 on any host
  but workpiece.dev, before the rest), `session.ts` (Turnstile, signed
  cookie), `turn.ts` (one model turn, its answer signed for the session),
  `status.ts`, and `tools.json` (the tools the browser runs: `run_python`,
  `view`, `read_guide`, `get_part`).
  Its code is in `lib/agent/`, its limits in `lib/agent/config.ts`, its
  system prompt in `lib/agent/prompt.ts`.
- `budget-worker/` — the worker `workpiece-agent-budget` with the Durable
  Object `AgentBudget` that enforces the spend caps, quotas and rates (its
  own `wrangler.toml`, with the caps). The Pages project binds it as
  `AGENT_BUDGET` through `script_name`, so it is deployed first.
- `dev-stub.mjs` — a stand-in for the endpoints with a canned model, serving
  the built app (`website/app/build.sh`) at `/app/`.
- `scripts/mock-anthropic.mjs` — a stand-in for the Messages API, for
  `wrangler pages dev` without a key.

See `../docs/assistant.md` for the architecture, the contract, the limits,
the secrets, the deploy and local development.
