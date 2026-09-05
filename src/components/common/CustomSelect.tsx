import { useState, useRef, useEffect, useId, type ReactNode } from "react";
import { Check, ChevronDown } from "lucide-react";

export type SelectOption = {
  value: string;
  label: string;
  description?: string;
  badge?: string;
  icon?: ReactNode;
  disabled?: boolean;
};

type CustomSelectProps = {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
  prefixIcon?: ReactNode;
};

export function CustomSelect({
  value,
  options,
  onChange,
  disabled = false,
  placeholder = "Select...",
  className = "",
  prefixIcon,
}: CustomSelectProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState<number>(-1);
  const containerRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const selectId = useId();
  const listboxId = `${selectId}-listbox`;

  const selectedOption = options.find((opt) => opt.value === value);

  // Close when clicked outside
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [isOpen]);

  // Keep highlighted index in sync when opening
  useEffect(() => {
    if (isOpen) {
      const idx = options.findIndex((opt) => opt.value === value);
      setHighlightedIndex(idx >= 0 ? idx : 0);
    }
  }, [isOpen, options, value]);

  // Scroll highlighted into view
  useEffect(() => {
    if (isOpen && listRef.current && highlightedIndex >= 0) {
      const el = listRef.current.children[highlightedIndex] as HTMLElement | undefined;
      el?.scrollIntoView({ block: "nearest" });
    }
  }, [highlightedIndex, isOpen]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (disabled) return;

    if (!isOpen) {
      if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setIsOpen(true);
      }
      return;
    }

    switch (e.key) {
      case "Escape":
        e.preventDefault();
        setIsOpen(false);
        break;
      case "Tab":
        setIsOpen(false);
        break;
      case "ArrowDown": {
        e.preventDefault();
        let nextIndex = highlightedIndex + 1;
        while (nextIndex < options.length && options[nextIndex].disabled) {
          nextIndex++;
        }
        if (nextIndex < options.length) {
          setHighlightedIndex(nextIndex);
        }
        break;
      }
      case "ArrowUp": {
        e.preventDefault();
        let prevIndex = highlightedIndex - 1;
        while (prevIndex >= 0 && options[prevIndex].disabled) {
          prevIndex--;
        }
        if (prevIndex >= 0) {
          setHighlightedIndex(prevIndex);
        }
        break;
      }
      case "Enter":
      case " ": {
        e.preventDefault();
        const option = options[highlightedIndex];
        if (option && !option.disabled) {
          onChange(option.value);
          setIsOpen(false);
        }
        break;
      }
    }
  };

  const handleSelect = (option: SelectOption) => {
    if (option.disabled || disabled) return;
    onChange(option.value);
    setIsOpen(false);
  };

  return (
    <div
      ref={containerRef}
      className={`custom-select-container ${isOpen ? "open" : ""} ${disabled ? "disabled" : ""} ${className}`}
      onKeyDown={handleKeyDown}
      onBlur={(event) => {
        const nextFocusedElement = event.relatedTarget as Node | null;
        if (!event.currentTarget.contains(nextFocusedElement)) {
          setIsOpen(false);
        }
      }}
    >
      <button
        type="button"
        role="combobox"
        id={selectId}
        className="custom-select-trigger"
        onClick={() => !disabled && setIsOpen((prev) => !prev)}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-controls={listboxId}
        aria-activedescendant={
          isOpen && highlightedIndex >= 0
            ? `${listboxId}-option-${highlightedIndex}`
            : undefined
        }
        disabled={disabled}
      >
        <span className="trigger-content">
          {prefixIcon && <span className="trigger-icon">{prefixIcon}</span>}
          {selectedOption ? (
            <span className="trigger-label">
              <span className="label-text">{selectedOption.label}</span>
              {selectedOption.badge && <span className="trigger-badge">{selectedOption.badge}</span>}
            </span>
          ) : (
            <span className="trigger-placeholder">{placeholder}</span>
          )}
        </span>
        <ChevronDown size={16} className={`chevron-icon ${isOpen ? "rotated" : ""}`} />
      </button>

      {isOpen && (
        <div className="custom-select-popover">
          <ul
            ref={listRef}
            id={listboxId}
            className="custom-select-options"
            role="listbox"
            tabIndex={-1}
          >
            {options.map((option, index) => {
              const isSelected = option.value === value;
              const isHighlighted = index === highlightedIndex;

              return (
                <li
                  key={option.value}
                  id={`${listboxId}-option-${index}`}
                  role="option"
                  aria-selected={isSelected}
                  aria-disabled={option.disabled}
                  className={`custom-select-option ${isSelected ? "selected" : ""} ${isHighlighted ? "highlighted" : ""} ${option.disabled ? "disabled" : ""}`}
                  onClick={() => handleSelect(option)}
                  onMouseEnter={() => !option.disabled && setHighlightedIndex(index)}
                >
                  <div className="option-main">
                    {option.icon && <span className="option-icon">{option.icon}</span>}
                    <div className="option-text">
                      <span className="option-label">{option.label}</span>
                      {option.description && (
                        <span className="option-description">{option.description}</span>
                      )}
                    </div>
                  </div>

                  <div className="option-trailing">
                    {option.badge && <span className="option-badge">{option.badge}</span>}
                    {isSelected && <Check size={14} className="selected-check" />}
                  </div>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </div>
  );
}
