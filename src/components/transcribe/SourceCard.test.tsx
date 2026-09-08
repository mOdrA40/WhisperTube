import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { SourceCard } from "./SourceCard";

describe("SourceCard", () => {
  it("exposes a named URL input and lets users cancel metadata inspection", () => {
    const onCancel = vi.fn();
    render(
      <I18nProvider>
        <SourceCard
          url="https://www.youtube.com/watch?v=test"
          busy
          inspecting
          cancelling={false}
          metadata={null}
          hasResult={false}
          onUrlChange={vi.fn()}
          onInspect={vi.fn()}
          onCancel={onCancel}
          onClear={vi.fn()}
          onOpenSettings={vi.fn()}
        />
      </I18nProvider>,
    );

    expect(screen.getByRole("textbox", { name: "Video URL" })).toBeTruthy();
    const cancelButton = screen.getByRole("button", { name: "Cancel check" });
    expect(cancelButton).not.toHaveProperty("disabled", true);
    fireEvent.click(cancelButton);
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
