import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { I18nProvider, getProgressMessage, useI18n } from "./i18n";

describe("resume progress copy", () => {
  it.each([
    ["en", "Resuming", "completed progress", "after completion"],
    ["id", "Melanjutkan", "disimpan otomatis", "setelah selesai"],
    ["zh", "继续", "自动保存", "完成后"],
  ] as const)("explains saved progress and audio retention in %s", (language, resume, checkpoint, retention) => {
    const { result } = renderHook(useI18n, { wrapper: I18nProvider });
    act(() => result.current.setLanguage(language));
    const progress = { stage: "transcribing" as const, backend: "cpu", message: "native raw message" };
    expect(getProgressMessage({ ...progress, messageCode: "resuming_transcription" }, "auto", result.current.t)).toContain(resume);
    expect(getProgressMessage({ ...progress, messageCode: "checkpoint_progress" }, "auto", result.current.t)).toContain(checkpoint);
    expect(result.current.t("controls.keepAudio")).toContain(retention);
  });
});
