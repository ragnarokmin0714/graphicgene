import { useState } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { EditorMode, Rgba } from "@/editor";
import { Header } from "@/Header";
import { LayerPanel } from "@/LayerPanel";
import { MOD, type Shortcut, useShortcuts } from "@/shortcuts";
import { Stage } from "@/Stage";
import { StatusBar } from "@/StatusBar";
import { type Tool, ToolDock } from "@/ToolDock";
import { useEditor } from "@/useEditor";

const WIDTH = 800;
const HEIGHT = 600;
const PROJECT_KEY = "graphicgene:project";
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
 */
export function App() {
  const { editor, revision, error, run } = useEditor(WIDTH, HEIGHT);
  const [tool, setTool] = useState<Tool>("select");
  const [notice, setNotice] = useState<string | null>(null);

  const core = editor.current;
  const layerCount = core ? core.layers().length : 0;
  const selectionCount = core?.selectionCount ?? 0;
  const mode = core?.mode ?? null;
  const nextFill = () => SWATCHES[layerCount % SWATCHES.length];

  const undo = () => run((editor) => editor.undo());
  const redo = () => run((editor) => editor.redo());

  // Core hands back bytes; where they live is the app layer's problem.
  const save = () =>
    run((editor) => {
      localStorage.setItem(PROJECT_KEY, editor.toJson());
      setNotice(`Saved ${timeFormat.format(new Date())}`);
    });

  const load = () =>
    run((editor) => {
      const text = localStorage.getItem(PROJECT_KEY);
      if (!text) {
        setNotice("Nothing saved yet");
        return;
      }
      editor.loadJson(text);
      setNotice("Loaded saved project");
    });

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
    { key: "s", mod: true, run: save },
    ...arrows,
  ]);

  return (
    <TooltipProvider delayDuration={400}>
      <div className="flex h-full flex-col">
        <Header onSave={save} onLoad={load} />

        {error && (
          <p
            className="bg-destructive/10 text-destructive border-destructive/20 border-b px-3 py-1.5"
            role="alert"
          >
            {error}
          </p>
        )}

        <main className="flex min-h-0 flex-1">
          <section className="bg-canvas-backdrop bg-dot-grid relative min-w-0 flex-1">
            <Stage
              editor={editor}
              revision={revision}
              run={run}
              width={WIDTH}
              height={HEIGHT}
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
          <LayerPanel
            editor={editor}
            revision={revision}
            onSelect={(id, additive) => run((editor) => editor.selectLayer(id, additive))}
          />
        </main>

        <StatusBar
          width={WIDTH}
          height={HEIGHT}
          layerCount={layerCount}
          selectionCount={selectionCount}
          hint={hintFor(tool, mode)}
          notice={notice}
        />
      </div>
    </TooltipProvider>
  );
}
