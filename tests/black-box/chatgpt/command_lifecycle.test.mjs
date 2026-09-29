import assert from "node:assert/strict";
import test from "node:test";

import {
  drivePublicCommandToTerminal,
  publicCommandIsPending,
  settleAcceptedPublicCommand,
} from "./command_lifecycle.mjs";

function response({ status, errorCode, retryable } = {}) {
  return {
    body: {
      result: {
        structuredContent: {
          data: status ? { status } : null,
          error: errorCode
            ? { code: errorCode, ...(retryable === undefined ? {} : { retryable }) }
            : null,
        },
      },
    },
  };
}

test("terminal driver treats transport budget expiry and running as non-terminal", async () => {
  const sequence = [
    response({ errorCode: "OperationTimedOut" }),
    response({ status: "running" }),
    response({ status: "cancelled" }),
  ];
  const calls = [];
  const terminal = await drivePublicCommandToTerminal({
    callTool: async (...args) => {
      calls.push(args);
      return sequence.shift();
    },
    publicSessionId: "lb-session-test",
    requestPrefix: "terminal",
  });

  assert.equal(terminal.body.result.structuredContent.data.status, "cancelled");
  assert.equal(calls.length, 3);
  assert.deepEqual(
    calls.map((call) => call[2]),
    ["terminal-0", "terminal-1", "terminal-2"],
  );
  assert.deepEqual(calls[0][1], {
    action: "poll",
    session_id: "lb-session-test",
    wait_ms: 100,
  });
});

test("terminal driver keeps polling retryable transport transients on the same session", async () => {
  const transient = response({ errorCode: "SessionUnavailable", retryable: true });
  const busy = response({ errorCode: "RuntimeUnavailable", retryable: true });
  assert.equal(publicCommandIsPending(transient), true);
  assert.equal(publicCommandIsPending(busy), true);
  const sequence = [transient, busy, response({ status: "completed" })];
  const calls = [];
  const terminal = await drivePublicCommandToTerminal({
    callTool: async (...args) => {
      calls.push(args);
      return sequence.shift();
    },
    publicSessionId: "lb-session-test",
    requestPrefix: "transient",
  });
  assert.equal(terminal.body.result.structuredContent.data.status, "completed");
  assert.deepEqual(
    calls.map((call) => call[2]),
    ["transient-0", "transient-1", "transient-2"],
  );
});

test("terminal driver returns typed tool errors instead of retrying them", async () => {
  const denied = response({ errorCode: "SessionUnavailable" });
  assert.equal(publicCommandIsPending(denied), false);
  // A non-retryable session error (an unknown or unowned session answers with
  // retryable: false) must surface immediately, not poll forever.
  const unknownSession = response({ errorCode: "SessionUnavailable", retryable: false });
  assert.equal(publicCommandIsPending(unknownSession), false);
  assert.equal(
    await drivePublicCommandToTerminal({
      callTool: async () => denied,
      publicSessionId: "lb-session-test",
      requestPrefix: "terminal",
    }),
    denied,
  );
});

test("terminal driver has one explicit wall-clock deadline", async () => {
  let now = 0;
  await assert.rejects(
    drivePublicCommandToTerminal({
      callTool: async () => response({ errorCode: "OperationTimedOut" }),
      publicSessionId: "lb-session-test",
      requestPrefix: "terminal",
      deadlineMs: 2,
      monotonicNow: () => now++,
    }),
    { code: "CommandTerminalDeadlineExceeded" },
  );
});

test("accepted command settlement reuses its stable public session identity", async () => {
  const initial = response({ status: "running" });
  initial.body.result.structuredContent.data.session_id = "lb-session-initial";
  const completed = response({ status: "completed" });
  const calls = [];
  assert.equal(
    await settleAcceptedPublicCommand({
      initialResponse: initial,
      callTool: async (...args) => {
        calls.push(args);
        return completed;
      },
      requestPrefix: "settle",
    }),
    completed,
  );
  assert.equal(calls[0][1].session_id, "lb-session-initial");
});

test("already terminal first responses settle without polling", async () => {
  const completed = response({ status: "completed" });
  const calls = [];
  assert.equal(
    await settleAcceptedPublicCommand({
      initialResponse: completed,
      callTool: async (...args) => {
        calls.push(args);
        return completed;
      },
      requestPrefix: "settle",
    }),
    completed,
  );
  assert.equal(calls.length, 0);
});
