import { X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { EditorMode, Rgba } from "@/editor";
import { download } from "@/files";
import { Header } from "@/Header";
import { LayerPanel } from "@/LayerPanel";
import { MOD, type Shortcut, useShortcuts } from "@/shortcuts";
import { Stage } from "@/Stage";
import { StatusBar } from "@/StatusBar";
import { readProject, writeProject } from "@/storage";
import { type Tool, ToolDock } from "@/ToolDock";
import { useEditor } from "@/useEditor";

/**
 * The artboard for a brand-new document. After that the size belongs to the
 * document — a loaded file brings its own — so it is read back from the core.
 */
const NEW_ARTBOARD = { width: 800, height: 600 };
const PROJECT_FILE = "graphicgene-project.json";
const SVG_FILE = "graphicgene.svg";
/** Quiet time after the last edit before autosaving, in ms. */
const AUTOSAVE_DELAY = 800;
/** Arrow-key nudge, and with Shift held, in document units. */
const NUDGE = 1;
const NUDGE_LARGE = 10;

/** Fills for new shapes, cycled so a fresh canvas is not a wall of one colour. */
const SWATCHES: readonly Rgba[] = [
  [99, 102, 241, 255],
  [244, 114, 94, 255],
  [20, 184, 166, 255],
  [245, 176, 65, 255],
  [217, 70, 239, 255],
];

const timeFormat = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });

/** Status-bar guidance for tools and modes whose controls are not visible. */
function hintFor(tool: Tool, mode: EditorMode | null): string | null {
  if (mode === "pen") {
    return `Click the first point to close · Enter or Esc to finish · ${MOD}+Z removes the last point`;
  }
  if (mode === "path") {
    return "Drag points and handles · Click a segment to add a point · Double-click a point for corner/curve · Enter to finish";
  }
  switch (tool) {
    case "pen":
      return "Click to add a point · Drag to pull out a curve";
    case "rect":
    case "ellipse":
      return "Drag to draw · Shift for equal sides · Alt from the centre";
    default:
      return null;
  }
}

/**
 * v0.1 shell. Every handler sends a command into the core and re-reads what
 * came back; nothing here mirrors document state. Selection lives in the core
 * as well, because it drives transforms.
 *
 * What React does keep is view state: the active tool, and `notice`, a
 * status-bar message about the last save or load.
 *
 * Persistence is app-layer IO: the core hands over JSON, and this component
 * autosaves it to IndexedDB after each pause in editing, restores it on the
 * next visit, and moves project and SVG files in and out of the browser.
 */
export function App() {
  const { editor, revision, error, clearError, run, ready } = useEditor(
    NEW_ARTBOARD.width,
    NEW_ARTBOARD.height,
  );
  const [tool, setTool] = useState<Tool>("select");
  const [notice, setNotice] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  /**
   * Autosave stays off until the stored project has been read back — or
   * for good, if it could not be loaded, so an empty document never
   * overwrites a project this build failed to open.
   */
  const restored = useRef(false);
  const lastSaved = useRef<string | null>(null);

  const core = editor.current;
  const width = core?.width ?? NEW_ARTBOARD.width;
  const height = core?.height ?? NEW_ARTBOARD.height;
  // Rows are re-read only when the core says they may have changed — never
  // during a drag, which is when this component re-renders every frame.
  const layersVersion = core?.layersVersion ?? null;
  const layers = useMemo(
    () => (core && layersVersion !== null ? core.layers() : []),
    [core, layersVersion],
  );
  const layerCount = layers.length;
  const selectionCount = core?.selectionCount ?? 0;
  const mode = core?.mode ?? null;
  const nextFill = () => SWATCHES[layerCount % SWATCHES.length];

  const selectLayer = useCallback(
    (id: string, additive: boolean) => run((editor) => editor.selectLayer(id, additive)),
    [run],
  );

  const undo = () => run((editor) => editor.undo());
  const redo = () => run((editor) => editor.redo());

  /** Write the project to IndexedDB unless unchanged; `force` reports even then. */
  const save = async (force = false) => {
    const core = editor.current;
    if (!core || !restored.current || core.busy) return;
    const json = core.toJson();
    if (json === lastSaved.current && !force) return;
    try {
      await writeProject(json);
      lastSaved.current = json;
      setNotice(`Saved ${timeFormat.format(new Date())}`);
    } catch {
      setNotice("Could not save in this browser");
    }
  };

  // Restore the last session once the core is up.
  useEffect(() => {
    if (!ready) return;
    let cancelled = false;
    readProject()
      .then((text) => {
        if (cancelled) return;
        if (text) {
          const loaded = run((editor) => {
            editor.loadJson(text);
            return true;
          });
          if (!loaded) {
            setNotice("Autosave paused: the saved project could not be opened");
            return;
          }
          lastSaved.current = text;
          setNotice("Restored your last session");
        }
        restored.current = true;
      })
      .catch(() => {
        if (!cancelled) setNotice("Autosave is unavailable in this browser");
      });
    return () => {
      cancelled = true;
    };
  }, [ready, run]);

  // Autosave after each pause in editing. A press in progress is skipped;
  // its release bumps the revision and schedules another try.
  useEffect(() => {
    if (!ready) return;
    const timer = setTimeout(() => void save(), AUTOSAVE_DELAY);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revision, ready]);

  // Leaving the tab is the last reliable moment to write.
  useEffect(() => {
    const onHide = () => {
      if (document.visibilityState === "hidden") void save();
    };
    document.addEventListener("visibilitychange", onHide);
    return () => document.removeEventListener("visibilitychange", onHide);
  });

  const openFile = async (file: File) => {
    const text = await file.text();
    const opened = run((editor) => {
      editor.loadJson(text);
      return true;
    });
    if (opened) setNotice(`Opened ${file.name}`);
  };

  const downloadProject = () =>
    run((editor) => download(PROJECT_FILE, editor.toJson(), "application/json"));

  const exportSvg = () =>
    run((editor) => download(SVG_FILE, editor.exportSvg(), "image/svg+xml"));

  /** Switching tools finishes a pen path or path edit in progress. */
  const changeTool = (next: Tool) => {
    run((editor) => editor.finishMode());
    setTool(next);
  };

  /**
   * Escape backs out one level: the drag, then the pen path or path edit
   * (kept, not discarded), then the tool, then the selection.
   */
  const escape = () =>
    run((editor) => {
      if (editor.cancelGesture()) return;
      if (editor.finishMode()) {
        if (tool === "pen") setTool("select");
        return;
      }
      if (tool !== "select") setTool("select");
      else editor.clearSelection();
    });

  /** Enter finishes the pen path or path edit, or starts editing the selected path. */
  const enter = () =>
    run((editor) => {
      if (editor.finishMode()) {
        if (tool === "pen") setTool("select");
      } else if (tool === "select") {
        editor.beginPathEdit();
      }
    });

  const nudge = (dx: number, dy: number) => run((editor) => editor.nudge(dx, dy));
  const arrows: Shortcut[] = (
    [
      ["arrowleft", -1, 0],
      ["arrowright", 1, 0],
      ["arrowup", 0, -1],
      ["arrowdown", 0, 1],
    ] as const
  ).flatMap(([key, x, y]) => [
    { key, repeat: true, run: () => nudge(x * NUDGE, y * NUDGE) },
    { key, shift: true, repeat: true, run: () => nudge(x * NUDGE_LARGE, y * NUDGE_LARGE) },
  ]);

  useShortcuts([
    { key: "v", run: () => changeTool("select") },
    { key: "r", run: () => changeTool("rect") },
    { key: "o", run: () => changeTool("ellipse") },
    { key: "p", run: () => changeTool("pen") },
    { key: "escape", run: escape },
    { key: "enter", run: enter },
    { key: "delete", run: () => run((editor) => editor.deleteSelection()) },
    { key: "backspace", run: () => run((editor) => editor.deleteSelection()) },
    { key: "a", mod: true, run: () => run((editor) => editor.selectAll()) },
    { key: "z", mod: true, run: undo },
    { key: "z", mod: true, shift: true, run: redo },
    { key: "y", mod: true, run: redo },
    { key: "s", mod: true, run: () => void save(true) },
    { key: "o", mod: true, run: () => fileInput.current?.click() },
    { key: "e", mod: true, shift: true, run: exportSvg },
    ...arrows,
  ]);

  return (
    <TooltipProvider delayDuration={400}>
      <div className="flex h-full flex-col">
        <Header
          onOpen={() => fileInput.current?.click()}
          onDownload={downloadProject}
          onExport={exportSvg}
        />
        <input
          ref={fileInput}
          type="file"
          accept=".json,application/json"
          hidden
          onChange={(event) => {
            const file = event.target.files?.[0];
            // Reset so choosing the same file again still fires.
            event.target.value = "";
            if (file) void openFile(file);
          }}
        />

        {error && (
          <div
            className="bg-destructive/10 text-destructive border-destructive/20 flex items-center gap-2 border-b py-1 pr-1 pl-3"
            role="alert"
          >
            <p className="min-w-0 flex-1 truncate">{error}</p>
            <Button size="icon-xs" onClick={clearError} aria-label="Dismiss">
              <X />
            </Button>
          </div>
        )}

        <main className="flex min-h-0 flex-1">
          <section className="bg-canvas-backdrop bg-dot-grid relative min-w-0 flex-1">
            <Stage
              editor={editor}
              revision={revision}
              run={run}
              width={width}
              height={height}
              tool={tool}
              nextFill={nextFill}
              onShapeDrawn={() => setTool("select")}
            />
            <ToolDock
              tool={tool}
              onToolChange={changeTool}
              onUndo={undo}
              onRedo={redo}
              canUndo={core?.canUndo ?? false}
              canRedo={core?.canRedo ?? false}
            />
          </section>
          <LayerPanel layers={layers} onSelect={selectLayer} />
        </main>

        <StatusBar
          width={width}
          height={height}
          layerCount={layerCount}
          selectionCount={selectionCount}
          hint={hintFor(tool, mode)}
          notice={notice}
        />
      </div>
    </TooltipProvider>
  );
}
