import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { CustomSelect } from "./CustomSelect";

describe("CustomSelect", () => {
  afterEach(() => cleanup());

  beforeEach(() => {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: () => undefined,
    });
  });

  it("exposes its semantic label and emits the selected value", () => {
    const onChange = vi.fn();
    render(
      <CustomSelect
        value="auto"
        options={[
          { value: "auto", label: "Auto detect" },
          { value: "cpu", label: "CPU" },
        ]}
        onChange={onChange}
        ariaLabel="Compute backend"
      />,
    );

    const trigger = screen.getByRole("combobox", { name: "Compute backend" });
    expect(trigger.getAttribute("aria-controls")).toBeTruthy();

    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("option", { name: "CPU" }));
    expect(onChange).toHaveBeenCalledWith("cpu");
  });

  it("supports keyboard navigation and typeahead without selecting disabled options", () => {
    const onChange = vi.fn();
    render(
      <CustomSelect
        value="auto"
        options={[
          { value: "auto", label: "Auto detect" },
          { value: "disabled", label: "CPU legacy", disabled: true },
          { value: "cpu", label: "CPU" },
        ]}
        onChange={onChange}
        ariaLabel="Compute backend"
      />,
    );

    const trigger = screen.getByRole("combobox", { name: "Compute backend" });
    fireEvent.click(trigger);
    fireEvent.keyDown(trigger, { key: "c" });
    fireEvent.keyDown(trigger, { key: "Enter" });

    expect(onChange).toHaveBeenCalledWith("cpu");
    expect(screen.queryByRole("option", { name: "CPU legacy" })).toBeNull();
  });

  it("preserves the highlighted option when the parent recreates equivalent options", () => {
    const onChange = vi.fn();
    function SelectWithParentRenders() {
      const [, rerenderParent] = useState(0);
      const [selected, setSelected] = useState("auto");
      return (
        <>
          <button onClick={() => rerenderParent((revision) => revision + 1)}>Rerender parent</button>
          <button onClick={() => setSelected("cpu")}>Select CPU externally</button>
          <CustomSelect
            value={selected}
            options={[
              { value: "auto", label: "Auto detect" },
              { value: "cpu", label: "CPU" },
            ]}
            onChange={(value) => {
              onChange(value);
              setSelected(value);
            }}
            ariaLabel="Compute backend"
          />
        </>
      );
    }

    render(<SelectWithParentRenders />);
    const trigger = screen.getByRole("combobox", { name: "Compute backend" });
    fireEvent.click(trigger);
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    fireEvent.click(screen.getByRole("button", { name: "Rerender parent" }));
    const listboxId = trigger.getAttribute("aria-controls");
    expect(trigger.getAttribute("aria-activedescendant")).toBe(`${listboxId}-option-1`);
    fireEvent.keyDown(trigger, { key: "ArrowUp" });
    fireEvent.click(screen.getByRole("button", { name: "Select CPU externally" }));
    expect(trigger.getAttribute("aria-activedescendant")).toBe(`${listboxId}-option-1`);
    fireEvent.keyDown(trigger, { key: "Enter" });

    expect(onChange).toHaveBeenCalledWith("cpu");
  });
});
