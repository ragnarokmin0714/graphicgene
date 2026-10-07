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
  /** Only in this set of shortcuts; in every set when left out. */
  keymap?: Keymap;
  run: () => void;
};

/**
 * Which set of shortcuts is in use where they disagree: graphicgene's own,
 * close to Figma's, or Illustrator's. Ctrl+D duplicates in one and
 * transforms again in the other, Ctrl+0 is 100% or fit, Ctrl+Y redoes or
 * shows outlines.
 */
export type Keymap = "default" | "illustrator";

/**
 * The key a shortcut means by an event: `key` lower-cased — or, when an
 * input method or a layout such as Zhuyin turns a letter or digit key into
 * something else ("Process", "ㄒ"), that key's place on a US keyboard, so
 * single-key tools keep working with Chinese input switched on.
 */
export function keyOf(event: KeyboardEvent): string {
  const key = event.key.toLowerCase();
  if (key.length === 1 && key >= " " && key <= "~") return key;
  const place = /^(?:Key|Digit)(.)$/.exec(event.code);
  return place ? place[1].toLowerCase() : key;
}

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
export function useShortcuts(shortcuts: readonly Shortcut[], keymap: Keymap = "default") {
  const latest = useRef(shortcuts);
  const latestKeymap = useRef(keymap);
  useLayoutEffect(() => {
    latest.current = shortcuts;
    latestKeymap.current = keymap;
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
      const key = keyOf(event);
      const hit = latest.current.find(
        (s) =>
          (s.keymap === undefined || s.keymap === latestKeymap.current) &&
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
