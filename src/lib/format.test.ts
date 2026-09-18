import { describe, expect, it } from "vitest";
import { errorCode, formatElapsedDuration, friendlyError } from "./format";

describe("structured backend errors", () => {
  it("reads a Tauri error payload without exposing its protocol shape", () => {
    const error = { code: "media_source_cookie_file", detail: "file is unavailable" };
    expect(errorCode(error)).toBe("media_source_cookie_file");
    expect(friendlyError(error)).toBe("file is unavailable");
  });

  it("keeps compatibility with legacy prefixed errors", () => {
    const error = "media_source_duration:video is too long";
    expect(errorCode(error)).toBe("media_source_duration");
    expect(friendlyError(error)).toBe("media_source_duration:video is too long");
  });
});

describe("formatElapsedDuration", () => {
  it("formats elapsed time as minutes and seconds", () => {
    expect(formatElapsedDuration(0)).toBe("00:00");
    expect(formatElapsedDuration(65_000)).toBe("01:05");
    expect(formatElapsedDuration(3_661_000)).toBe("1:01:01");
  });
});
