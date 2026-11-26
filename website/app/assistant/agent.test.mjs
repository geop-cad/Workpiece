import assert from "node:assert/strict";
import { test } from "node:test";
// The assistant's loop (agent.js), ported with it from geop's web/test/agent.test.ts.
// Run by `npm test` in cloud/ (node --test).
import {
  AgentError,
  EARLIER_PICTURE,
  answer,
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

const said = (text, stop = "end_turn") => ({
  message: { content: [{ type: "text", text }], stop_reason: stop },
  limits: { turns_left_today: 9, turns_per_day: 10 },
});
const uses = (...calls) => ({
  message: {
    content: [
      { type: "thinking", thinking: "plan", signature: "sig" },
      ...calls.map(([id, name, input]) => ({ type: "tool_use", id, name, input })),
    ],
    stop_reason: "tool_use",
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

test("a tool use cut off by max_tokens is left out of the conversation", async () => {
  const cut = {
    message: {
      content: [
        { type: "text", text: "Adding" },
        { type: "tool_use", id: "a", name: "run_python", input: {} },
      ],
      stop_reason: "max_tokens",
    },
  };
  const result = await answer(
    withPrompt([], "x"),
    { turn: server([cut]).turn, runTool: async () => assert.fail("not run") },
    new AbortController().signal,
  );
  assert.equal(result.ended, "max_tokens");
  assert.deepEqual(result.messages[1].content, [{ type: "text", text: "Adding" }]);
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
