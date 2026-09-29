import assert from "node:assert/strict";
import test from "node:test";
import { requireRetainedOutputReference } from "./output_reference.mjs";

const response = (data) => ({ transport: { status: 200 }, body: { result: {
  isError: true, structuredContent: { ok: false, error: { code: "ProcessFailed" },
    warnings: ["test-warning"], data },
} } });

test("missing replay references fail with all three command responses", () => {
  const initial = response({ session_id: "lb-session-test", status: "running" });
  const terminal = response({ session_id: "lb-session-test", status: "failed", output: "marker" });
  const replay = response({ session_id: "lb-session-test", status: "failed", truncated: true });
  assert.throws(() => requireRetainedOutputReference({ initial, terminal, replay, stream: "stderr", requestId: "stderr-cached-terminal" }),
    (error) => ["stderr-cached-terminal", "lb-session-test", "initial", "terminal", "replay", "test-warning", "truncated"].every((value) => error.message.includes(value))
      && !error.message.includes("TypeError"));
});

test("retained references must be nonempty public handles", () => {
  for (const value of [undefined, null, "", 3, "private-stderr"]) {
    const replay = response({ output_refs: { stderr: value } });
    assert.throws(() => requireRetainedOutputReference({ replay, stream: "stderr", requestId: "replay" }));
  }
  const replay = response({ output_refs: { stdout: "lb-output-out", stderr: "lb-output-err" } });
  assert.equal(requireRetainedOutputReference({ replay, stream: "stderr", requestId: "replay" }), "lb-output-err");
});
