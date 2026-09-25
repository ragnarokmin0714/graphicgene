import { useState } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Canvas } from "@/Canvas";
import type { Rgba } from "@/editor";
import { Header } from "@/Header";
import { LayerPanel } from "@/LayerPanel";
import { useShortcuts } from "@/shortcuts";
import { StatusBar } from "@/StatusBar";
import { ToolDock } from "@/ToolDock";
import { useEditor } from "@/useEditor";

const WIDTH = 800;
const HEIGHT = 600;
const PROJECT_KEY = "graphicgene:project";

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
 * came back; nothing here mirrors document state.
 *
 * `selected` is the exception that proves the rule — it is view state (which
 * row is highlighted), not document state, so it lives in React. When
 * selection starts driving transforms it moves into the core. `notice` is the
 * same kind of state: a status-bar message about the last save or load.
 */
export function App() {
  const { editor, revision, error, run } = useEditor(WIDTH, HEIGHT);
  const [selected, setSelected] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const core = editor.current;
  const layerCount = core ? core.layers().length : 0;
  const swatch = () => SWATCHES[layerCount % SWATCHES.length];

  const addRect = () =>
    run((editor) =>
      setSelected(
        editor.addRect(80 + Math.random() * 480, 60 + Math.random() * 360, 160, 120, swatch()),
      ),
    );

  const addEllipse = () =>
    run((editor) =>
      setSelected(
        editor.addEllipse(160 + Math.random() * 480, 140 + Math.random() * 320, 80, 80, swatch()),
      ),
    );

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
      setSelected(null);
      setNotice("Loaded saved project");
    });

  useShortcuts([
    { key: "r", run: addRect },
    { key: "o", run: addEllipse },
    { key: "z", mod: true, run: undo },
    { key: "z", mod: true, shift: true, run: redo },
    { key: "y", mod: true, run: redo },
    { key: "s", mod: true, run: save },
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
            <Canvas editor={editor} revision={revision} width={WIDTH} height={HEIGHT} />
            <ToolDock
              onAddRect={addRect}
              onAddEllipse={addEllipse}
              onUndo={undo}
              onRedo={redo}
              canUndo={core?.canUndo ?? false}
              canRedo={core?.canRedo ?? false}
            />
          </section>
          <LayerPanel
            editor={editor}
            revision={revision}
            selected={selected}
            onSelect={setSelected}
          />
        </main>

        <StatusBar width={WIDTH} height={HEIGHT} layerCount={layerCount} notice={notice} />
      </div>
    </TooltipProvider>
  );
}
