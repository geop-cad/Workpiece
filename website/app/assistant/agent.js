// The assistant's loop, without the page: what is sent to the server
// (`/api/agent/*`, see cloud/ and docs/assistant.md), and how the tools the
// model asks for are run and answered. The server owns the model, its
// instructions and its tools; the page sends the conversation, as the
// Anthropic Messages API has it, and runs the tools (assistant.js, on the
// Python layer in worker.js and the kernel).
//
// Ported from geop's web/src/agent.ts. Kept free of the DOM and the kernel,
// so that agent.test.mjs runs it with a fetch and tools of its own.

/**
 * Why the server refused: `session` (the session ended: prove again to be
 * human), `rate` (too fast), `quota` (the user's allowance for today is
 * used up), `budget` (the assistant's is, or it is off), `invalid`,
 * `upstream` (the model failed), `network` (no answer at all).
 */
export class AgentError extends Error {
  constructor(code, message, status, retryAfter = null) {
    super(message);
    this.name = "AgentError";
    this.code = code;
    this.status = status;
    this.retryAfter = retryAfter;
  }
}

/** Whether `e` means the assistant cannot be used again today: the user's allowance, or everybody's, is spent. */
export function usedUp(e) {
  return e instanceof AgentError && (e.code === "quota" || e.code === "budget");
}

/** POSTs `body` to `path` as JSON: the answer's JSON, or the server's refusal as an AgentError. */
async function post(fetchFn, path, body, signal) {
  let response;
  try {
    response = await fetchFn(path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      credentials: "same-origin",
      body: JSON.stringify(body),
      signal,
    });
  } catch (e) {
    if (signal?.aborted) throw e;
    throw new AgentError("network", `The assistant could not be reached: ${String(e)}`, 0);
  }
  const text = await response.text();
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {
    // Not JSON: a proxy's error page, say.
  }
  if (!response.ok) {
    const refusal = json ?? {};
    throw new AgentError(
      refusal.error ?? (response.status === 401 ? "session" : "upstream"),
      refusal.message ?? `The assistant answered ${response.status}.`,
      response.status,
      refusal.retry_after ?? null,
    );
  }
  if (json == null) throw new AgentError("upstream", "The assistant answered something that is not JSON.", response.status);
  return json;
}

/** Start a session with the Turnstile `token`: the cookie the server sets, and the limits it tells. */
export async function startSession(token, base = "", fetchFn = fetch) {
  const answer = await post(fetchFn, `${base}/api/agent/session`, { turnstile: token });
  return answer.limits ?? null;
}

/** One turn of the model on `messages`. */
export function postTurn(messages, signal, base = "", fetchFn = fetch) {
  return post(fetchFn, `${base}/api/agent/turn`, { messages }, signal);
}

/** Whether the assistant takes turns now (`GET /api/agent/status`); false when it cannot be asked. */
export async function available(base = "", fetchFn = fetch) {
  try {
    const response = await fetchFn(`${base}/api/agent/status`, { credentials: "same-origin" });
    if (!response.ok) return false;
    return (await response.json()).available === true;
  } catch {
    return false;
  }
}

/** The most turns of the model one prompt may take: a loop that never ends costs the user's allowance. */
export const MAX_TURNS = 25;

/** What a stopped answer tells the model of the tools it asked for and did not get. */
const NOT_RUN = "Not run: the user stopped this answer.";

/** What an earlier picture is sent as: the server takes one picture a request, and the latest is the part as it is. */
export const EARLIER_PICTURE = "[An earlier picture: look again to see the part now.]";

/**
 * The conversation as sent: every picture but the latest a tool answered
 * with said in words instead (EARLIER_PICTURE). The conversation kept is
 * left as it is.
 */
export function withLatestPicture(messages) {
  let seen = false;
  const kept = [...messages].reverse().map((message) => {
    const content = [...message.content].reverse().map((block) => {
      if (block.type !== "tool_result" || typeof block.content === "string") return block;
      const parts = [...block.content].reverse().map((part) => {
        if (part.type !== "image") return part;
        if (!seen) {
          seen = true;
          return part;
        }
        return { type: "text", text: EARLIER_PICTURE };
      });
      return { ...block, content: parts.reverse() };
    });
    return { ...message, content: content.reverse() };
  });
  return kept.reverse();
}

/**
 * Answer the conversation `messages`, which ends with the user's prompt:
 * turn after turn of the model, running every tool it asks for and
 * answering all of them in one message, until it stops on its own, the
 * user stops it (`signal`), or it took `maxTurns`. The conversation as it
 * then is — always one the next prompt can follow, every tool use
 * answered — and why it ended. A refusal of the server is thrown, the
 * conversation up to it told through `loop.onMessage`.
 *
 * `loop`: `turn(messages, signal)` (one turn of the model), `runTool(name,
 * input, signal)` ({text, isError, images?: [{media_type, data}]}), and
 * optionally `onMessage(message)`, `onToolStart(use)`, `onTool(use,
 * result)`, `onLimits(limits)`.
 */
export async function answer(messages, loop, signal, maxTurns = MAX_TURNS) {
  const history = [...messages];
  const add = (message) => {
    history.push(message);
    loop.onMessage?.(message);
  };
  for (let turn = 0; turn < maxTurns; turn++) {
    if (signal.aborted) return { messages: history, ended: "stopped" };
    let answered;
    try {
      answered = await loop.turn(withLatestPicture(history), signal);
    } catch (e) {
      if (signal.aborted) return { messages: history, ended: "stopped" };
      throw e;
    }
    if (answered.limits) loop.onLimits?.(answered.limits);
    const { content, stop_reason } = answered.message;
    const uses = content.filter((b) => b.type === "tool_use");
    if (stop_reason !== "tool_use" || uses.length === 0) {
      // A tool use cut off (`max_tokens`) is never answered: left out, so
      // that the conversation can go on.
      const kept = content.filter((b) => b.type !== "tool_use");
      add({ role: "assistant", content: kept.length > 0 ? kept : [{ type: "text", text: "…" }] });
      if (stop_reason === "pause_turn") continue;
      return { messages: history, ended: stop_reason ?? "end_turn" };
    }
    add({ role: "assistant", content });
    const results = [];
    for (const use of uses) {
      let result;
      if (signal.aborted) result = { text: NOT_RUN, isError: true };
      else {
        loop.onToolStart?.(use);
        try {
          result = await loop.runTool(use.name, use.input, signal);
        } catch (e) {
          result = { text: `${use.name} failed: ${e instanceof Error ? e.message : String(e)}`, isError: true };
        }
        loop.onTool?.(use, result);
      }
      const images = result.images ?? [];
      results.push({
        type: "tool_result",
        tool_use_id: use.id,
        content:
          images.length === 0
            ? result.text
            : [{ type: "text", text: result.text }, ...images.map((image) => ({ type: "image", source: { type: "base64", ...image } }))],
        ...(result.isError ? { is_error: true } : {}),
      });
    }
    add({ role: "user", content: results });
  }
  return { messages: history, ended: signal.aborted ? "stopped" : "turns" };
}

/**
 * The conversation `messages` with the user's prompt `text` added — and,
 * before it, `context` (what the part open is, as the model has not seen
 * it). After tool results the prompt joins their message: the
 * conversation alternates between the user and the model.
 */
export function withPrompt(messages, text, context = []) {
  const blocks = [...context, { type: "text", text }];
  const last = messages[messages.length - 1];
  if (last?.role === "user") return [...messages.slice(0, -1), { role: "user", content: [...last.content, ...blocks] }];
  return [...messages, { role: "user", content: blocks }];
}

/**
 * A short note on the part open in the editor (`document`: {path, steps,
 * script}), sent with a prompt when the model has not seen it as it is:
 * the model reads the rest with get_part.
 */
export function partContext(document) {
  if (!document) return { type: "text", text: "No part is open in the editor: the first part a script builds opens." };
  const made = document.script ? "It was made by a script (get_part shows it)." : "No script made it: the user built it by hand.";
  return {
    type: "text",
    text: `The part open in the editor is ${document.path}, with ${document.steps} steps. ${made}`,
  };
}

/** What a tool did, in a line: `Ran a script — 2 steps failed`. */
export function describeTool(name, input, result) {
  const args = input ?? {};
  let said;
  switch (name) {
    case "run_python":
      said = "Ran a script";
      break;
    case "view":
      said = args.sheet ? `Looked at the sheet \`${String(args.sheet)}\`` : `Looked at the part from the ${String(args.side ?? "iso")}`;
      break;
    case "read_guide":
      said = args.section ? `Read the guide: ${String(args.section)}` : "Read the guide's contents";
      break;
    case "get_part":
      said = "Read the open part";
      break;
    default:
      said = `Used \`${name}\``;
  }
  if (result.isError) return `${said}: ${firstLine(result.text)}`;
  const failed = /^(\d+) of \d+ steps failed$/m.exec(result.text ?? "");
  const n = failed ? Number(failed[1]) : 0;
  return n > 0 ? `${said} — ${n} step${n === 1 ? "" : "s"} failed` : said;
}

/** What went wrong, in a line. */
function firstLine(text) {
  const line = (text ?? "").trim().split("\n").filter(Boolean).pop() ?? "";
  return line.length > 160 ? `${line.slice(0, 160)}…` : line;
}
