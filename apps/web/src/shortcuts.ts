import { useEffect, useLayoutEffect, useRef } from "react";

const isMac = /Mac|iPhone|iPad/.test(navigator.userAgent);

/** Modifier labels as the user's platform prints them. */
export const MOD = isMac ? "⌘" : "Ctrl";
export const SHIFT = isMac ? "⇧" : "Shift";

export type Shortcut = {
  /** `KeyboardEvent.key`, lower-cased. */
  key?: string;
  /**
   * `KeyboardEvent.code`, for keys whose `key` depends on the layout and on
   * Shift — Shift+1 is "!" on one keyboard and something else on the next.
   */
  code?: string;
  mod?: boolean;
  shift?: boolean;
  /** Keep firing while the key is held. Defaults to true only for modified shortcuts. */
  repeat?: boolean;
  run: () => void;
};

/**
 * Window-level keyboard shortcuts.
 *
 * Ignored while focus is in a text field, so the text tool and property
 * inputs can take the same keys, and for keys a control has already handled
 * (`preventDefault`): the Escape that closes a popover must not also
 * deselect. By default auto-repeat only fires modified shortcuts: holding
 * Ctrl+Z steps back through history, holding Delete must not keep deleting.
 * Arrow nudges opt in with `repeat`.
 */
export function useShortcuts(shortcuts: readonly Shortcut[]) {
  const latest = useRef(shortcuts);
  useLayoutEffect(() => {
    latest.current = shortcuts;
  });

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // A key something already handled — Escape closing a menu, or ending
      // a scrub — is not a shortcut as well.
      if (event.defaultPrevented) return;
      if (
        event.target instanceof Element &&
        event.target.closest("input, textarea, select, [contenteditable]")
      ) {
        return;
      }
      const mod = event.metaKey || event.ctrlKey;
      const key = event.key.toLowerCase();
      const hit = latest.current.find(
        (s) =>
          (s.key === key || (s.code !== undefined && s.code === event.code)) &&
          !!s.mod === mod &&
          !!s.shift === event.shiftKey &&
          !event.altKey,
      );
      if (!hit) return;
      event.preventDefault();
      if (event.repeat && !(hit.repeat ?? hit.mod)) return;
      hit.run();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
