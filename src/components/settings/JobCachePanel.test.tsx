import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { JobCachePanel } from "./JobCachePanel";

afterEach(cleanup);
const entry = { id: "job-id", title: "Interrupted video", sourceUrl: "https://example.test/video", bytes: 1024, modifiedAt: "2026-10-22T00:00:00Z" };
describe("selective saved-progress cleanup", () => {
  it("requires confirmation, preserves progress on cancel and reports failed deletion", async () => {
    const onDiscard = vi.fn().mockRejectedValue(new Error("File sedang terkunci"));
    const onLoad = vi.fn().mockResolvedValue(undefined);
    render(<I18nProvider><JobCachePanel items={[entry]} busy={false} onLoad={onLoad} onDiscard={onDiscard} /></I18nProvider>);
    expect(screen.getByText("Interrupted video")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Review unfinished jobs" }));
    expect(onLoad).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Discard saved progress" }));
    expect(onDiscard).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Keep progress" }));
    expect(onDiscard).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Discard saved progress" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete progress permanently" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("File sedang terkunci"));
    expect(onDiscard).toHaveBeenCalledWith(entry.id);
    expect(screen.getByRole("button", { name: "Delete progress permanently" })).toBeTruthy();
    onDiscard.mockResolvedValue(undefined);
    fireEvent.click(screen.getByRole("button", { name: "Delete progress permanently" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Keep progress" })).toBeNull());
  });

  it("disables cache reads and deletion while another operation owns storage", () => {
    render(<I18nProvider><JobCachePanel items={[entry]} busy={true} onLoad={vi.fn()} onDiscard={vi.fn()} /></I18nProvider>);
    for (const button of screen.getAllByRole("button")) expect((button as HTMLButtonElement).disabled).toBe(true);
  });
});
