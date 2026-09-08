import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import type { HistoryItem } from "../../types";
import { HistoryPage } from "./HistoryPage";

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
  });
});
