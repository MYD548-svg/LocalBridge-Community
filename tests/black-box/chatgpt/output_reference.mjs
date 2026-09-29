import assert from "node:assert/strict";

export function requireRetainedOutputReference({ initial, terminal, replay, stream, requestId }) {
  // Keep diagnostics confined to command responses; never include client headers.
  const diagnostic = JSON.stringify({ requestId, stream, initial, terminal, replay }, null, 2);
  const refs = replay?.body?.result?.structuredContent?.data?.output_refs;
  assert.ok(refs && typeof refs === "object" && !Array.isArray(refs), `missing retained output references: ${diagnostic}`);
  const value = refs[stream];
  assert.ok(typeof value === "string" && value.startsWith("lb-output-") && value.length > "lb-output-".length,
    `invalid retained ${stream} reference: ${diagnostic}`);
  return value;
}
