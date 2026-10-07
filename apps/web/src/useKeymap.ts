import { useEffect, useState } from "react";
import type { Keymap } from "@/shortcuts";

const STORAGE_KEY = "graphicgene:keymap";

function readKeymap(): Keymap {
  try {
    if (localStorage.getItem(STORAGE_KEY) === "illustrator") return "illustrator";
  } catch {
    // Storage can be unavailable (private mode, blocked site data).
  }
  return "default";
}

/**
 * The set of shortcuts in use. A per-viewer preference like the theme, so
 * it lives in React and localStorage, not in the document; "default" is
 * stored as the absence of a value.
 */
export function useKeymap() {
  const [keymap, setKeymap] = useState<Keymap>(readKeymap);
  useEffect(() => {
    try {
      if (keymap === "default") localStorage.removeItem(STORAGE_KEY);
      else localStorage.setItem(STORAGE_KEY, keymap);
    } catch {
      // Not persisting is fine; the choice still applies for this session.
    }
  }, [keymap]);
  return { keymap, setKeymap };
}
