import { useCallback, useEffect, useRef, useState } from "react";
import { EditorHandle } from "@/editor";

/**
 * Holds the editor instance and a revision counter.
 *
 * The counter is the only "state" React keeps: it says *that* the document
 * changed, never *what* it now contains. Views re-read from the core.
 */
export function useEditor(width: number, height: number) {
  const handle = useRef<EditorHandle | null>(null);
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    EditorHandle.create(width, height)
      .then((editor) => {
        if (cancelled) return;
        handle.current = editor;
        setRevision((r) => r + 1);
      })
      .catch((e: unknown) => setError(String(e)));
    return () => {
      cancelled = true;
    };
    // Size changes go through resize(), not a fresh editor — a new editor
    // would discard the document.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Run one interaction against the core and mark the document changed. */
  const run = useCallback(<T,>(fn: (editor: EditorHandle) => T): T | undefined => {
    const editor = handle.current;
    if (!editor) return undefined;
    try {
      const result = fn(editor);
      setRevision((r) => r + 1);
      return result;
    } catch (e: unknown) {
      setError(String(e));
      return undefined;
    }
  }, []);

  return { editor: handle, revision, error, run, ready: handle.current !== null };
}
