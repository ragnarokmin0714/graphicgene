import { useState } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { Rgba } from "@/editor";
import { Header } from "@/Header";
import { LayerPanel } from "@/LayerPanel";
import { type Shortcut, useShortcuts } from "@/shortcuts";
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

  /** Escape backs out one level: the drag, then the shape tool, then the selection. */
  const escape = () =>
    run((editor) => {
      if (editor.cancelGesture()) return;
      if (tool !== "select") setTool("select");
      else editor.clearSelection();
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
    { key: "v", run: () => setTool("select") },
    { key: "r", run: () => setTool("rect") },
    { key: "o", run: () => setTool("ellipse") },
    { key: "escape", run: escape },
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
              onToolChange={setTool}
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
          notice={notice}
        />
      </div>
    </TooltipProvider>
  );
}
