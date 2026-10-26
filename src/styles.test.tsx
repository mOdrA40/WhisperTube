import styles from "./styles.css?inline";
import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";

it("retains the focus-visible outline on the transcript search input", () => {
  const style = document.createElement("style");
  style.textContent = styles;
  try {
    render(<div className="transcript-search"><input aria-label="Search transcript" /></div>);
    const input = screen.getByRole("textbox");
    input.focus();
    expect(input.matches(":focus-visible")).toBe(true);
    // jsdom does not invalidate its computed-style cache on focus changes.
    // Attach after focus to evaluate the real selectors without a stale cache.
    document.head.append(style);
    expect(getComputedStyle(input).outline).toContain("2px solid");
  } finally { style.remove(); }
});
