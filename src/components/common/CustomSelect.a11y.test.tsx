import axe from "axe-core";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CustomSelect } from "./CustomSelect";

describe("CustomSelect accessibility", () => {
  it("has no axe violations when opened", async () => {
    const { container } = render(
      <CustomSelect
        value="auto"
        options={[
          { value: "auto", label: "Automatic" },
          { value: "cpu", label: "CPU" },
        ]}
        onChange={() => undefined}
        ariaLabel="Compute backend"
      />,
    );

    fireEvent.click(screen.getByRole("combobox"));
    const results = await axe.run(container, {
      rules: {
        "color-contrast": { enabled: false },
      },
    });

    expect(results.violations).toEqual([]);
  });
});
