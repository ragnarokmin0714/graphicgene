import { useState } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Canvas } from "@/Canvas";
import { LayerPanel } from "@/LayerPanel";
import { Toolbar } from "@/Toolbar";
import { useEditor } from "@/useEditor";

const WIDTH = 800;
const HEIGHT = 600;

/**
 * v0.1 shell. Every handler sends a command into the core and re-reads what
 * came back; nothing here mirrors document state.
 *
 * `selected` is the exception that proves the rule — it is view state (which
 * row is highlighted), not document state, so it lives in React. When
 * selection starts driving transforms it moves into the core.
 */
export function App() {
  const { editor, revision, error, run } = useEditor(WIDTH, HEIGHT);
  const [selected, setSelected] = useState<string | null>(null);

  const core = editor.current;

  const addRect = () =>
    run((editor) =>
      setSelected(
        editor.addRect(
          40 + Math.random() * 400,
          40 + Math.random() * 300,
          160,
          120,
          [80, 120, 240, 255],
        ),
      ),
    );

  const addEllipse = () =>
    run((editor) =>
      setSelected(
        editor.addEllipse(
          120 + Math.random() * 400,
          120 + Math.random() * 300,
          90,
          60,
          [240, 120, 80, 255],
        ),
      ),
    );

  return (
    <TooltipProvider delayDuration={400}>
      <div className="flex h-full flex-col">
        <Toolbar
          onAddRect={addRect}
          onAddEllipse={addEllipse}
          onUndo={() => run((editor) => editor.undo())}
          onRedo={() => run((editor) => editor.redo())}
          // Core hands back bytes; where they live is the app layer's problem.
          onSave={() =>
            run((editor) => localStorage.setItem("graphicgene:project", editor.toJson()))
          }
          onLoad={() =>
            run((editor) => {
              const text = localStorage.getItem("graphicgene:project");
              if (text) editor.loadJson(text);
            })
          }
          canUndo={core?.canUndo ?? false}
          canRedo={core?.canRedo ?? false}
        />

        {error && (
          <p className="bg-destructive/10 text-destructive px-2 py-1" role="alert">
            {error}
          </p>
        )}

        <main className="flex min-h-0 flex-1">
          <Canvas editor={editor} revision={revision} width={WIDTH} height={HEIGHT} />
          <LayerPanel
            editor={editor}
            revision={revision}
            selected={selected}
            onSelect={setSelected}
          />
        </main>
      </div>
    </TooltipProvider>
  );
}
