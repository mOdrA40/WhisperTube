import { useLayoutEffect, type RefObject } from "react";

// Covers both body portals and dialogs nested in a page. Only siblings along
// the dialog's ancestry become inert, so the dialog never disables itself.
export function useModalFocus(ref: RefObject<HTMLElement | null>, open: boolean) {
  useLayoutEffect(() => {
    const dialog = ref.current;
    if (!open || !dialog) return;
    const focusable = () => [...dialog.querySelectorAll<HTMLElement>(
      "button, a[href], input, select, textarea, [tabindex]",
    )].filter((element) => element.tabIndex >= 0 && !element.matches(":disabled, input[type='hidden']") &&
      !element.closest("[hidden], [inert]") &&
      (!element.matches("input[type='radio']") || (element as HTMLInputElement).checked));
    const focusInside = () => {
      const active = document.activeElement;
      if (!active || !dialog.contains(active) || active.matches(":disabled") || active.closest("[hidden], [inert]")) {
        (focusable()[0] ?? dialog).focus({ preventScroll: true });
      }
    };
    const inertElements = new Map<HTMLElement, string | null>();
    let branch: HTMLElement = dialog;
    while (branch.parentElement) {
      for (const sibling of branch.parentElement.children) {
        if (sibling !== branch && sibling instanceof HTMLElement) {
          inertElements.set(sibling, sibling.getAttribute("inert"));
          sibling.setAttribute("inert", "");
        }
      }
      if (branch.parentElement === document.body) break;
      branch = branch.parentElement;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const controls = focusable();
      const first = controls[0];
      const last = controls[controls.length - 1];
      const active = document.activeElement;
      if (!first || !controls.some((control) => control === active)) {
        event.preventDefault();
        ((event.shiftKey ? last : first) ?? dialog).focus({ preventScroll: true });
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus({ preventScroll: true });
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus({ preventScroll: true });
      }
    };
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("focusin", focusInside);
    // Disabling the focused button may move native focus to body without a
    // focusin event. Recover after disabled/fieldset/child changes as well.
    const observer = new MutationObserver(focusInside);
    observer.observe(dialog, { attributes: true, attributeFilter: ["disabled", "hidden", "tabindex"], childList: true, subtree: true });
    focusInside();
    return () => {
      observer.disconnect();
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("focusin", focusInside);
      for (const [element, previous] of inertElements) {
        if (previous === null) element.removeAttribute("inert");
        else element.setAttribute("inert", previous);
      }
    };
  }, [ref, open]);
}
