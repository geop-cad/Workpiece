import assert from "node:assert/strict";
import { test } from "node:test";
// The assistant's loop (agent.js), ported with it from geop's web/test/agent.test.ts.
// Run by `npm test` in cloud/ (node --test).
import { validateTurn } from "../../../cloud/lib/agent/validate.ts";
import {
  AgentError,
  COMPACT_AT_BYTES,
  EARLIER_PICTURE,
  LONGEST_TOOL_TEXT,
  answer,
  compact,
  sizeOf,
  toolText,
  describeTool,
  partContext,
  postTurn,
  usedUp,
  withPrompt,
} from "./agent.js";

/** A server answering turns with `answers`, one after another, and what it was sent. */
function server(answers) {
  const sent = [];
  return {
    sent,
    turn: async (messages) => {
      sent.push(structuredClone(messages));
      const next = answers.shift();
      if (!next) throw new Error("asked one turn too many");
      return next;
    },
  };
}

const SIG = "S".repeat(43);
const said = (text, stop = "end_turn") => ({
  message: { content: [{ type: "text", text }], stop_reason: stop, signature: SIG },
  limits: { turns_left_today: 9, turns_per_day: 10 },
});
const uses = (...calls) => ({
  message: {
    content: [
      { type: "thinking", thinking: "plan", signature: "sig" },
      ...calls.map(([id, name, input]) => ({ type: "tool_use", id, name, input })),
    ],
    stop_reason: "tool_use",
    signature: SIG,
  },
});

test("every tool asked for runs, and all are answered in one message, until the model stops", async () => {
  const model = server([
    uses(["a", "read_guide", { section: "quick" }], ["b", "run_python", { script: "print(1)" }]),
    said("Done: a box."),
  ]);
  const ran = [];
  const result = await answer(
    withPrompt([], "a box"),
    {
      turn: model.turn,
      runTool: async (name) => {
        ran.push(name);
        return { text: name === "run_python" ? "1 of 2 steps failed" : "the guide", isError: false };
      },
    },
    new AbortController().signal,
  );
  assert.deepEqual(ran, ["read_guide", "run_python"]);
  assert.equal(result.ended, "end_turn");
  assert.equal(model.sent.length, 2);
  const second = model.sent[1];
  // The thinking block went back unchanged; the results came in one user message.
  assert.deepEqual(second[1].content[0], { type: "thinking", thinking: "plan", signature: "sig" });
  assert.equal(second[2].role, "user");
  assert.deepEqual(
    second[2].content.map((b) => (b.type === "tool_result" ? b.tool_use_id : b.type)),
    ["a", "b"],
  );
  assert.equal(result.messages.length, 4);
});

test("a tool that fails is answered as an error, and the loop goes on", async () => {
  const model = server([uses(["a", "get_part", {}]), said("It did not exist.")]);
  const result = await answer(
    withPrompt([], "remove x"),
    {
      turn: model.turn,
      runTool: async () => {
        throw new Error("the kernel crashed");
      },
    },
    new AbortController().signal,
  );
  const results = result.messages[2].content[0];
  assert.equal(results.type, "tool_result");
  assert.equal(results.type === "tool_result" && results.is_error, true);
  assert.equal(result.ended, "end_turn");
});

test("an answer takes no more turns than the cap, and ends answerable", async () => {
  const forever = Array.from({ length: 5 }, (_, i) => uses([`t${i}`, "run_python", { script: "" }]));
  const result = await answer(
    withPrompt([], "loop"),
    { turn: server(forever).turn, runTool: async () => ({ text: "{}", isError: false }) },
    new AbortController().signal,
    3,
  );
  assert.equal(result.ended, "turns");
  // The last message answers the last tool use: the next prompt joins it.
  const last = result.messages[result.messages.length - 1];
  assert.equal(last.role, "user");
  const next = withPrompt(result.messages, "continue");
  assert.equal(next.length, result.messages.length);
  assert.deepEqual(next[next.length - 1].content.at(-1), { type: "text", text: "continue" });
});

test("stopping answers the tools not run, and asks no further turn", async () => {
  const controller = new AbortController();
  const model = server([uses(["a", "run_python", { script: "" }], ["b", "run_python", { script: "" }]), said("never")]);
  let ran = 0;
  const result = await answer(
    withPrompt([], "go"),
    {
      turn: model.turn,
      runTool: async () => {
        ran++;
        controller.abort();
        return { text: "{}", isError: false };
      },
    },
    controller.signal,
  );
  assert.equal(ran, 1);
  assert.equal(result.ended, "stopped");
  assert.equal(model.sent.length, 1);
  const answered = result.messages[2].content;
  assert.equal(answered.length, 2, "both tool uses answered");
});

test("an assistant message is kept exactly as the server answered it, with its signature", async () => {
  const cut = { message: { content: [{ type: "text", text: "Adding" }], stop_reason: "max_tokens", signature: SIG } };
  const result = await answer(
    withPrompt([], "x"),
    { turn: server([cut]).turn, runTool: async () => assert.fail("not run") },
    new AbortController().signal,
  );
  assert.equal(result.ended, "max_tokens");
  assert.deepEqual(result.messages[1], { role: "assistant", content: [{ type: "text", text: "Adding" }], signature: SIG });
});

test("a tool's text is never empty and never over what the server takes", () => {
  assert.equal(toolText(""), "(no output)");
  assert.equal(toolText(undefined), "(no output)");
  const long = toolText("x".repeat(LONGEST_TOOL_TEXT + 500));
  assert.equal(long.length, LONGEST_TOOL_TEXT);
  assert.match(long, /cut off\]$/);
});

/** A conversation of `rounds` script runs, each script and report `size` characters. */
function longConversation(rounds, size) {
  let messages = withPrompt([], "Make a bracket.", [{ type: "text", text: "No part is open in the editor." }]);
  for (let i = 0; i < rounds; i++) {
    messages.push({
      role: "assistant",
      signature: SIG,
      content: [
        { type: "thinking", thinking: "plan", signature: "sig" },
        { type: "text", text: `Try ${i}.` },
        { type: "tool_use", id: `toolu_${i}`, name: "run_python", input: { script: `# run ${i}\n${"p.extrude(s, 8)\n".repeat(size / 16)}` } },
      ],
    });
    messages.push({ role: "user", content: [{ type: "tool_result", tool_use_id: `toolu_${i}`, content: `report ${i}\n${"ok Extrude\n".repeat(size / 11)}` }] });
  }
  return messages;
}

test("a conversation grown past COMPACT_AT_BYTES is compacted before it is sent, into one the server takes", async () => {
  const history = longConversation(12, 6000);
  assert.ok(sizeOf(history) > COMPACT_AT_BYTES);
  const model = server([said("Done.")]);
  const compacted = [];
  const result = await answer(history, { turn: model.turn, runTool: async () => assert.fail(), onCompact: (h) => compacted.push(h) }, new AbortController().signal);
  assert.equal(compacted.length, 1);
  const sent = model.sent[0];
  assert.equal(sent.length, 1);
  assert.ok(sizeOf(sent) < COMPACT_AT_BYTES / 2);
  const summary = sent[0].content[0].text;
  assert.match(summary, /compacted/);
  assert.match(summary, /Make a bracket/);
  assert.match(summary, /# run 11/);
  assert.match(summary, /report 11/);
  // The server's own validation takes it: no assistant message, every text within its limit.
  validateTurn({ messages: sent }, sizeOf(sent), new Set(["run_python"]));
  assert.equal(result.messages.length, 2);
});

test("compacting keeps the user's latest words as they are", () => {
  const history = withPrompt(longConversation(2, 160), "Now make it 10 mm thicker.");
  const [only] = compact(history);
  assert.equal(only.role, "user");
  assert.deepEqual(only.content.at(-1), { type: "text", text: "Now make it 10 mm thicker." });
});

test("too_long compacts and asks again, once", async () => {
  const history = longConversation(2, 160);
  const sent = [];
  let calls = 0;
  const result = await answer(
    history,
    {
      turn: async (messages) => {
        sent.push(messages);
        if (calls++ === 0) throw new AgentError("too_long", "too long", 400);
        return said("Done.");
      },
      runTool: async () => assert.fail(),
    },
    new AbortController().signal,
  );
  assert.equal(result.ended, "end_turn");
  assert.equal(sent[1].length, 1);
  // Twice too long: the error stands.
  await assert.rejects(
    answer(history, { turn: async () => { throw new AgentError("too_long", "too long", 400); }, runTool: async () => assert.fail() }, new AbortController().signal),
    (e) => e.code === "too_long",
  );
});

test("a session that ended is renewed once, and the conversation, signed for the old one, is compacted", async () => {
  const history = longConversation(1, 160);
  let renewed = 0;
  let calls = 0;
  const sent = [];
  const result = await answer(
    history,
    {
      turn: async (messages) => {
        sent.push(messages);
        if (calls++ === 0) throw new AgentError("session", "ended", 401);
        return said("Done.");
      },
      renewSession: async () => void renewed++,
      runTool: async () => assert.fail(),
    },
    new AbortController().signal,
  );
  assert.equal(renewed, 1);
  assert.equal(result.ended, "end_turn");
  assert.ok(sent[1].every((m) => m.role === "user"));
});

test("the server's refusals are read from its JSON", async () => {
  const refusing = (status, body) => async () =>
    new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  await assert.rejects(
    postTurn([], undefined, "", refusing(429, { error: "quota", message: "used up" })),
    (e) => e instanceof AgentError && e.code === "quota" && usedUp(e),
  );
  await assert.rejects(
    postTurn([], undefined, "", refusing(429, { error: "rate", message: "slow down", retry_after: 3 })),
    (e) => e instanceof AgentError && e.code === "rate" && e.retryAfter === 3 && !usedUp(e),
  );
  await assert.rejects(
    postTurn([], undefined, "", async () => new Response("<html>", { status: 502 })),
    (e) => e instanceof AgentError && e.code === "upstream",
  );
  let body = null;
  const ok = await postTurn([{ role: "user", content: [{ type: "text", text: "hi" }] }], undefined, "", async (_, init) => {
    body = JSON.parse(String(init?.body));
    return new Response(JSON.stringify(said("hello")));
  });
  assert.equal(ok.message.stop_reason, "end_turn");
  assert.deepEqual(body, { messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }] });
});

test("a tool's card says what it did, and what failed", () => {
  assert.equal(describeTool("run_python", { script: "" }, { text: "ok x\n0 of 2 steps failed\n", isError: false }), "Ran a script");
  assert.equal(describeTool("run_python", { script: "" }, { text: "FAILED x\n2 of 5 steps failed\n", isError: false }), "Ran a script — 2 steps failed");
  assert.equal(
    describeTool("run_python", {}, { text: "Traceback (most recent call last):\n  ...\nNameError: name 'q' is not defined\n", isError: true }),
    "Ran a script: NameError: name 'q' is not defined",
  );
  assert.equal(describeTool("view", { sheet: "drawing1" }, { text: "", isError: false }), "Looked at the sheet `drawing1`");
  assert.equal(describeTool("view", {}, { text: "", isError: false }), "Looked at the part from the iso");
  assert.equal(describeTool("read_guide", { section: "drawing" }, { text: "", isError: false }), "Read the guide: drawing");
});

test("the note on the open part is short, and says whether a script made it", () => {
  assert.match(partContext(null).text, /No part is open/);
  assert.match(partContext({ path: "plate.wpc", steps: 3, script: "x" }).text, /plate\.wpc, with 3 steps\. It was made by a script/);
  assert.match(partContext({ path: "a.wpc", steps: 0, script: null }).text, /by hand/);
});

test("a tool's pictures reach the model, and only the latest is sent again", async () => {
  const model = server([
    uses(["a", "view", { side: "iso" }]),
    uses(["b", "view", { side: "top" }]),
    said("Done."),
  ]);
  const pictures = ["first", "second"];
  await answer(
    withPrompt([], "show me"),
    {
      turn: model.turn,
      runTool: async () => ({
        text: "{}",
        isError: false,
        images: [{ media_type: "image/png", data: pictures.shift() }],
      }),
    },
    new AbortController().signal,
  );
  const sentPictures = (messages) =>
    messages.flatMap((m) =>
      m.content.flatMap((b) =>
        b.type === "tool_result" && typeof b.content !== "string"
          ? b.content.map((part) => (part.type === "image" ? part.source.data : part.text))
          : [],
      ),
    );
  assert.deepEqual(sentPictures(model.sent[1]), ["{}", "first"]);
  assert.deepEqual(sentPictures(model.sent[2]), ["{}", EARLIER_PICTURE, "{}", "second"]);
});
