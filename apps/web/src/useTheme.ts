import { useEffect, useState } from "react";

export type ThemePreference = "light" | "dark" | "system";

/** Also read by the inline script in index.html, which runs before first paint. */
const STORAGE_KEY = "graphicgene:theme";

const darkQuery = () => window.matchMedia("(prefers-color-scheme: dark)");

function readPreference(): ThemePreference {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // Storage can be unavailable (private mode, blocked site data).
  }
  return "system";
}

function applyTheme(preference: ThemePreference) {
  const dark = preference === "dark" || (preference === "system" && darkQuery().matches);
  document.documentElement.classList.toggle("dark", dark);
}

/**
 * Chrome theme. This is a per-viewer preference, not document state, so it
 * lives in React and localStorage rather than in the core.
 *
 * "system" is stored as the absence of a value, so a user who never touched
 * the switch keeps following the OS.
 */
export function useTheme() {
  const [preference, setPreference] = useState<ThemePreference>(readPreference);

  useEffect(() => {
    applyTheme(preference);
    try {
      if (preference === "system") localStorage.removeItem(STORAGE_KEY);
      else localStorage.setItem(STORAGE_KEY, preference);
    } catch {
      // Not persisting is fine; the choice still applies for this session.
    }

    if (preference !== "system") return;
    const query = darkQuery();
    const onChange = () => applyTheme("system");
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, [preference]);

  return { preference, setPreference };
}
