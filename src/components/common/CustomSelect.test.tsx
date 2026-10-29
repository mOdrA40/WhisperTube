import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { CustomSelect } from "./CustomSelect";

describe("CustomSelect", () => {
  it("starts a fresh keyboard search after reopening", () => {
    const onChange = vi.fn();
    render(<CustomSelect value="cpu" options={[{value:"auto",label:"Auto"},{value:"cpu",label:"CPU"}]} onChange={onChange} ariaLabel="Backend" />);
    const trigger = screen.getByRole("combobox", {name:"Backend"});
    fireEvent.click(trigger);
    fireEvent.keyDown(trigger, {key:"c"});
    fireEvent.keyDown(trigger, {key:"Escape"});
    fireEvent.click(trigger);
    fireEvent.keyDown(trigger, {key:"a"});
    fireEvent.keyDown(trigger, {key:"Enter"});
    expect(onChange).toHaveBeenCalledWith("auto");
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

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

  it.each(["0", "10"])("scrolls the overflow owner for initial value %s and keyboard navigation", (value) => {
    // jsdom has no layout. Supply viewport geometry while exercising the real
    // DOM/ref owner and scroll algorithm, including scrolling back upward.
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      if (this.classList.contains("custom-select-popover")) return new DOMRect(0, 100, 100, 40);
      if (this.getAttribute("role") === "option") {
        const index = Array.from(this.parentElement!.children).indexOf(this);
        const owner = this.closest(".custom-select-popover")!;
        return new DOMRect(0, 100 + index * 20 - owner.scrollTop, 100, 20);
      }
      return new DOMRect();
    });
    vi.spyOn(Element.prototype, "clientHeight", "get").mockImplementation(function (this: Element) {
      return this.classList.contains("custom-select-popover") ? 40 : 400;
    });
    render(<CustomSelect value={value} options={Array.from({ length: 12 }, (_, index) => ({
      value: String(index), label: `Device ${index}`,
    }))} onChange={vi.fn()} ariaLabel="Device" />);
    const trigger = screen.getByRole("combobox");
    fireEvent.click(trigger);
    const list = screen.getByRole("listbox");
    const owner = list.parentElement!;
    expect(owner.scrollTop).toBe(value === "10" ? 180 : 0);
    fireEvent.keyDown(trigger, { key: "End" });
    expect(owner.scrollTop).toBe(200);
    expect(list.scrollTop).toBe(0);
    fireEvent.keyDown(trigger, { key: "Home" });
    expect(owner.scrollTop).toBe(0);
  });

  it("opens above a low trigger, stays below the titlebar, and repositions on resize", () => {
    let triggerBounds = new DOMRect(window.innerWidth - 160, window.innerHeight - 50, 150, 38);
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      if (this.classList.contains("custom-select-trigger")) return triggerBounds;
      if (this.classList.contains("window-titlebar")) return new DOMRect(0, 0, window.innerWidth, 36);
      return new DOMRect();
    });
    const { container } = render(<><div className="window-titlebar" /><CustomSelect value="auto"
      options={[{ value: "auto", label: "Automatic" }]} onChange={vi.fn()} ariaLabel="Device" /></>);
    fireEvent.click(screen.getByRole("combobox"));
    const popover = container.querySelector<HTMLElement>(".custom-select-popover")!;
    expect(popover.style.position).toBe("fixed");
    expect(popover.style.bottom).toBe("56px");
    expect(popover.style.maxHeight).toBe("240px");
    triggerBounds = new DOMRect(20, 70, 150, 38);
    fireEvent(window, new Event("resize"));
    expect(popover.style.top).toBe("114px");
    expect(popover.style.bottom).toBe("auto");
    expect(popover.style.left).toBe("20px");
    fireEvent.keyDown(screen.getByRole("combobox"), { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});
