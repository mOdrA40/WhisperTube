import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { SourceCard } from "./SourceCard";

describe("SourceCard", () => {
  it("discloses recovered MP3 audio before transcription", () => {
    render(<I18nProvider><SourceCard url="" busy={false} inspecting={false} cancelling={false} hasResult={false}
      metadata={{ id: "local", title: "recording.mp3", channel: "Local audio", duration: 60, thumbnail: null,
        webpageUrl: "file:///recording.mp3", availability: null, source: "Local audio", audioRecovered: true }}
      onUrlChange={vi.fn()} onInspect={vi.fn()} onCancel={vi.fn()} onClear={vi.fn()} onOpenSettings={vi.fn()} /></I18nProvider>);
    expect(screen.getByRole("status").textContent).toContain("damaged MP3 packets were skipped");
    expect(screen.getByRole("status").textContent).toContain("original file was not changed");
  });
  it("imports audio through the picker and blocks selection during processing", () => {
    const onImportAudio = vi.fn();
    const props = { url: "", busy: false, inspecting: false, cancelling: false, metadata: null, hasResult: false,
      onUrlChange: vi.fn(), onInspect: vi.fn(), onCancel: vi.fn(), onClear: vi.fn(), onOpenSettings: vi.fn(), onImportAudio };
    const { rerender } = render(<I18nProvider><SourceCard {...props} /></I18nProvider>);
    fireEvent.click(screen.getByRole("button", { name: "Choose local audio" }));
    expect(onImportAudio).toHaveBeenCalledOnce();
    rerender(<I18nProvider><SourceCard {...props} busy /></I18nProvider>);
    expect(screen.getByRole("button", { name: "Choose local audio" })).toHaveProperty("disabled", true);
  });
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
