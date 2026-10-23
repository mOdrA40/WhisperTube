import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import type { HistoryItem } from "../../types";
import { HistoryPage } from "./HistoryPage";
afterEach(cleanup);

function renderHistory(overrides: Partial<Parameters<typeof HistoryPage>[0]> = {}) {
  const props = { history: makeHistory(1), hasMore: false, loadingMore: false, operationActive: false,
    onRefresh: vi.fn(), onLoadMore: vi.fn(), onLoad: vi.fn(), onDelete: vi.fn().mockResolvedValue(undefined),
    onDeleteFiles: vi.fn().mockResolvedValue(undefined), onListFiles: vi.fn().mockResolvedValue([]),
    onRevealFile: vi.fn().mockResolvedValue(undefined), onTabChange: vi.fn(), ...overrides };
  render(<I18nProvider><HistoryPage {...props} /></I18nProvider>);
  return props;
}

function makeHistory(count: number): HistoryItem[] {
  return Array.from({ length: count }, (_, index) => ({
    id: index + 1,
    title: `Video ${index + 1}`,
    channel: "Test channel",
    sourceUrl: "https://www.youtube.com/watch?v=test",
    createdAt: "2026-01-01T00:00:00.000Z",
    duration: 60,
    language: "en",
    model: "base",
    backend: "cpu",
  }));
}

describe("HistoryPage", () => {
  it.each(["Saved files: Video 1", "Delete history: Video 1"])("opens %s outside the filtered card and restores focus and page scrolling", async (name) => {
    renderHistory();
    document.body.style.overflow = "auto";
    const trigger = screen.getByRole("button", { name });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog");
    expect(dialog.closest(".card")).toBeNull();
    expect(dialog.parentElement?.parentElement).toBe(document.body);
    expect(document.body.style.overflow).toBe("hidden");
    expect(dialog.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(document.body.style.overflow).toBe("auto");
    document.body.style.overflow = "";
  });

  it.each(["audio", "transcript"] as const)("requires confirmation and deletes only the chosen %s scope", async (scope) => {
    const props = renderHistory();
    fireEvent.click(screen.getByRole("button", { name: "Delete history: Video 1" }));
    fireEvent.click(screen.getByRole("radio", { name: scope === "audio" ? "Audio only — keep transcripts and history" : "Transcripts only — keep saved audio and history" }));
    expect(props.onDeleteFiles).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /^Delete$/ }));
    await waitFor(() => expect(props.onDeleteFiles).toHaveBeenCalledWith([1], scope));
    expect(props.onDelete).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Saved files: Video 1" })).toBeTruthy();
  });

  it("shows actual file sizes and locations and reveals the selected format", async () => {
    const props = renderHistory({ onListFiles: vi.fn().mockResolvedValue([
      { kind: "audio", path: "D:\\data\\job\\audio.wav", bytes: 32000, available: true, pendingDeletion: false },
      { kind: "txt", path: "D:\\data\\job\\transcript.txt", bytes: 100, available: true, pendingDeletion: false },
      { kind: "srt", path: "D:\\data\\job\\transcript.srt", bytes: null, available: false, pendingDeletion: false },
    ]) });
    fireEvent.click(screen.getByRole("button", { name: "Saved files: Video 1" }));
    await screen.findByText("D:\\data\\job\\audio.wav");
    expect(props.onListFiles).toHaveBeenCalledWith(1);
    expect(screen.getByText("D:\\data\\job\\transcript.txt")).toBeTruthy();
    const locations = screen.getAllByRole("button", { name: "Open file location" });
    expect((locations[2] as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(locations[1]);
    await waitFor(() => expect(props.onRevealFile).toHaveBeenCalledWith(1, "txt"));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("keeps confirmation open when the file deletion fails", async () => {
    const props = renderHistory({ onDeleteFiles: vi.fn().mockRejectedValue(new Error("Database locked")) });
    fireEvent.click(screen.getByRole("button", { name: "Delete history: Video 1" }));
    fireEvent.click(screen.getByRole("radio", { name: "Audio only — keep transcripts and history" }));
    fireEvent.click(screen.getByRole("button", { name: /^Delete$/ }));
    await waitFor(() => expect(props.onDeleteFiles).toHaveBeenCalled());
    expect(screen.getByRole("dialog")).toBeTruthy();
  });
  it("never sends more than 100 history IDs for bulk deletion", async () => {
    const onDelete = vi.fn().mockResolvedValue(undefined);
    render(
      <I18nProvider>
        <HistoryPage
          history={makeHistory(101)}
          hasMore={false}
          loadingMore={false}
          operationActive={false}
          onRefresh={vi.fn()}
          onLoadMore={vi.fn()}
          onLoad={vi.fn()}
          onDelete={onDelete}
          onDeleteFiles={vi.fn()}
          onListFiles={vi.fn()}
          onRevealFile={vi.fn()}
          onTabChange={vi.fn()}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Select all" }));
    expect(screen.getAllByRole("checkbox").filter((checkbox) => (checkbox as HTMLInputElement).checked)).toHaveLength(100);
    expect(screen.getByRole("checkbox", { name: "Select history item: Video 1" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Delete history: Video 1" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Delete selected" }));
    fireEvent.click(screen.getByRole("button", { name: /^Delete$/ }));
    await waitFor(() => expect(onDelete).toHaveBeenCalledWith(Array.from({ length: 100 }, (_, index) => index + 1)));
  }, 15_000);
});
