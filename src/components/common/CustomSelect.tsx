import { useState, useRef, useEffect, useLayoutEffect, useId, type CSSProperties, type ReactNode } from "react";
import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { CaretDownIcon } from "@phosphor-icons/react/dist/csr/CaretDown";

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
  ariaLabel?: string;
  ariaLabelledBy?: string;
};

export function CustomSelect({
  value,
  options,
  onChange,
  disabled = false,
  placeholder = "Select...",
  className = "",
  prefixIcon,
  ariaLabel,
  ariaLabelledBy,
}: CustomSelectProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState<number>(-1);
  const containerRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const [popoverStyle, setPopoverStyle] = useState<CSSProperties>();
  const shouldScrollHighlightedRef = useRef(false);
  const wasOpenRef = useRef(false);
  const previousOptionsRef = useRef(options);
  const previousValueRef = useRef(value);
  const typeaheadRef = useRef("");
  const typeaheadTimerRef = useRef<number | null>(null);
  const selectId = useId();
  const listboxId = `${selectId}-listbox`;

  const selectedOption = options.find((opt) => opt.value === value);

  // A fixed popover escapes the configuration panel's overflow without moving
  // the listbox out of its owning keyboard/focus subtree. Keep it in the window
  // when the scroll container or viewport changes, including text zoom.
  useLayoutEffect(() => {
    if (!isOpen) return;
    const position = () => {
      const trigger = triggerRef.current;
      if (!trigger) return;
      const bounds = trigger.getBoundingClientRect();
      const gap = 6;
      const margin = 8;
      const topMargin = Math.max(margin, (document.querySelector(".window-titlebar")?.getBoundingClientRect().bottom ?? 0) + margin);
      const below = window.innerHeight - bounds.bottom - gap - margin;
      const above = bounds.top - gap - topMargin;
      const opensAbove = below < 200 && above > below;
      const maxHeight = Math.max(36, Math.min(240, opensAbove ? above : below));
      const width = Math.min(bounds.width, window.innerWidth - margin * 2);
      setPopoverStyle({
        position: "fixed", width, right: "auto",
        left: Math.max(margin, Math.min(bounds.left, window.innerWidth - width - margin)),
        top: opensAbove ? "auto" : Math.max(topMargin, bounds.bottom + gap),
        bottom: opensAbove ? Math.max(margin, window.innerHeight - bounds.top + gap) : "auto",
        maxHeight,
      });
    };
    position();
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    return () => {
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
    };
  }, [isOpen]);

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

  // Keep the active option stable across equivalent parent renders that
  // recreate the options array while the user is navigating by keyboard.
  useLayoutEffect(() => {
    if (isOpen) {
      const selected = options.findIndex((option) => option.value === value && !option.disabled);
      const fallback = options.findIndex((option) => !option.disabled);
      if (!wasOpenRef.current || previousValueRef.current !== value) {
        const nextIndex = selected >= 0 ? selected : fallback;
        shouldScrollHighlightedRef.current = true;
        setHighlightedIndex(nextIndex);
      } else if (previousOptionsRef.current !== options) {
        const highlightedValue = previousOptionsRef.current[highlightedIndex]?.value;
        const preserved = options.findIndex(
          (option) => option.value === highlightedValue && !option.disabled,
        );
        const nextIndex = preserved >= 0 ? preserved : selected >= 0 ? selected : fallback;
        if (nextIndex !== highlightedIndex) {
          shouldScrollHighlightedRef.current = true;
          setHighlightedIndex(nextIndex);
        }
      }
    }
    wasOpenRef.current = isOpen;
    previousOptionsRef.current = options;
    previousValueRef.current = value;
  }, [highlightedIndex, isOpen, options, value]);

  // Scroll highlighted into view
  useEffect(() => {
    if (isOpen && shouldScrollHighlightedRef.current && listRef.current && scrollRef.current && highlightedIndex >= 0) {
      const el = listRef.current.children[highlightedIndex] as HTMLElement | undefined;
      if (el) {
        const list = scrollRef.current;
        // Rectangles share viewport coordinates even when the positioned
        // ancestor differs from the element that owns overflow scrolling.
        const viewport = list.getBoundingClientRect();
        const option = el.getBoundingClientRect();
        const top = option.top - viewport.top - list.clientTop + list.scrollTop;
        const bottom = top + option.height;
        if (top < list.scrollTop) list.scrollTop = top;
        else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
      }
      shouldScrollHighlightedRef.current = false;
    }
  }, [highlightedIndex, isOpen]);

  useEffect(() => {
    typeaheadRef.current = "";
    if (typeaheadTimerRef.current !== null) window.clearTimeout(typeaheadTimerRef.current);
    typeaheadTimerRef.current = null;
    return () => {
      if (typeaheadTimerRef.current !== null) window.clearTimeout(typeaheadTimerRef.current);
    };
  }, [isOpen]);

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
          shouldScrollHighlightedRef.current = true;
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
          shouldScrollHighlightedRef.current = true;
          setHighlightedIndex(prevIndex);
        }
        break;
      }
      case "Home": {
        e.preventDefault();
        const firstEnabled = options.findIndex((option) => !option.disabled);
        if (firstEnabled >= 0) {
          shouldScrollHighlightedRef.current = true;
          setHighlightedIndex(firstEnabled);
        }
        break;
      }
      case "End": {
        e.preventDefault();
        let lastEnabled = -1;
        for (let index = options.length - 1; index >= 0; index -= 1) {
          if (!options[index].disabled) {
            lastEnabled = index;
            break;
          }
        }
        if (lastEnabled >= 0) {
          shouldScrollHighlightedRef.current = true;
          setHighlightedIndex(lastEnabled);
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
      default: {
        if (e.key.length !== 1 || e.ctrlKey || e.metaKey || e.altKey) break;
        typeaheadRef.current = `${typeaheadRef.current}${e.key.toLowerCase()}`;
        if (typeaheadTimerRef.current !== null) window.clearTimeout(typeaheadTimerRef.current);
        typeaheadTimerRef.current = window.setTimeout(() => {
          typeaheadRef.current = "";
          typeaheadTimerRef.current = null;
        }, 500);
        const match = options.findIndex(
          (option) => !option.disabled && option.label.toLowerCase().startsWith(typeaheadRef.current),
        );
        if (match >= 0) {
          shouldScrollHighlightedRef.current = true;
          setHighlightedIndex(match);
        }
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
        ref={triggerRef}
        type="button"
        role="combobox"
        id={selectId}
        className="custom-select-trigger"
        onClick={() => !disabled && setIsOpen((prev) => !prev)}
        aria-haspopup="listbox"
        aria-disabled={disabled || undefined}
        aria-expanded={isOpen}
        aria-controls={listboxId}
        aria-label={ariaLabel}
        aria-labelledby={ariaLabelledBy}
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
        <CaretDownIcon aria-hidden="true" size={16} className={`chevron-icon ${isOpen ? "rotated" : ""}`} />
      </button>

      {isOpen && (
        <div ref={scrollRef} className="custom-select-popover" style={popoverStyle}>
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
                  tabIndex={-1}
                  aria-selected={isSelected}
                  aria-disabled={option.disabled}
                  className={`custom-select-option ${isSelected ? "selected" : ""} ${isHighlighted ? "highlighted" : ""} ${option.disabled ? "disabled" : ""}`}
                  onClick={() => handleSelect(option)}
                  onMouseEnter={() => {
                    shouldScrollHighlightedRef.current = false;
                    if (!option.disabled) setHighlightedIndex(index);
                  }}
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
                    {isSelected && <CheckIcon aria-hidden="true" size={14} className="selected-check" />}
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
