import assert from "node:assert/strict";
import { test } from "node:test";
import { validateInferenceResult } from "./verify-native-inference.mjs";

test("native smoke check requires real structured output, not just process exit zero", () => {
  validateInferenceResult({ transcription: [] });
  validateInferenceResult({ transcription: [{ text: "fixture", offsets: { from: 0, to: 1000 } }] });
  for (const result of [null, {}, { transcription: "not an array" }, { transcription: Array(101).fill({}) },
    { transcription: [{ text: "bad", offsets: { from: 10, to: 0 } }] },
    { transcription: [{ text: "bad", offsets: { from: 0, to: NaN } }] },
    { transcription: [{ text: 10, offsets: { from: 0, to: 1000 } }] }]) {
    assert.throws(() => validateInferenceResult(result));
  }
  assert.throws(() => validateInferenceResult({ transcription: [null] }), /invalid transcript segment/);
});
