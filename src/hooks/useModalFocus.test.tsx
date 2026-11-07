import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode, useRef } from "react";
import { describe, expect, it } from "vitest";
import { useModalFocus } from "./useModalFocus";

function Fixture({ open = true, disabled = false }: { open?: boolean; disabled?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useModalFocus(ref, open);
  return <section>
    <button>Background</button>
    <aside inert>Already inert</aside>
    {open && <div role="dialog" aria-label="Fixture" ref={ref} tabIndex={-1}>
      <fieldset disabled={disabled}><button>First</button><button>Last</button></fieldset>
    </div>}
  </section>;
}

describe("modal focus ownership", () => {
  it("wraps both directions, redirects outside focus and restores previous inert state", () => {
    const { rerender } = render(<StrictMode><Fixture /></StrictMode>);
    const dialog = screen.getByRole("dialog");
    const background = screen.getByText("Background");
    const alreadyInert = screen.getByText("Already inert");
    expect(background.hasAttribute("inert")).toBe(true);
    expect(document.activeElement).toBe(screen.getByText("First"));
    expect(fireEvent.keyDown(document.activeElement!, { key: "Tab", shiftKey: true })).toBe(false);
    expect(document.activeElement).toBe(screen.getByText("Last"));
    expect(fireEvent.keyDown(document.activeElement!, { key: "Tab" })).toBe(false);
    expect(document.activeElement).toBe(screen.getByText("First"));
    background.focus(); // jsdom does not implement inert; the focusin guard must still contain it.
    expect(dialog.contains(document.activeElement)).toBe(true);
    rerender(<StrictMode><Fixture open={false} /></StrictMode>);
    expect(background.hasAttribute("inert")).toBe(false);
    expect(alreadyInert.hasAttribute("inert")).toBe(true);
    background.focus();
    expect(document.activeElement).toBe(background);
  });

  it("focuses the dialog when inherited disabled controls leave no tab stop", async () => {
    const { rerender, unmount } = render(<Fixture />);
    const dialog = screen.getByRole("dialog");
    rerender(<Fixture disabled />);
    await waitFor(() => expect(document.activeElement).toBe(dialog));
    for (const shiftKey of [false, true]) {
      expect(fireEvent.keyDown(dialog, { key: "Tab", shiftKey })).toBe(false);
      expect(document.activeElement).toBe(dialog);
    }
    const background = screen.getByText("Background");
    unmount();
    expect(background.hasAttribute("inert")).toBe(false);
    expect(fireEvent.keyDown(document, { key: "Tab" })).toBe(true);
  });

  it("recovers focus when the focused controls become hidden", async () => {
    render(<Fixture />);
    const dialog = screen.getByRole("dialog");
    screen.getByText("First").closest("fieldset")!.hidden = true;
    await waitFor(() => expect(document.activeElement).toBe(dialog));
    expect(fireEvent.keyDown(dialog, { key: "Tab" })).toBe(false);
    expect(document.activeElement).toBe(dialog);
  });
});
