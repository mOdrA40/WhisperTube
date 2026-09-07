import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CustomSelect } from "./CustomSelect";

describe("CustomSelect", () => {
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
});
