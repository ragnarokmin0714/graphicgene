import { useEffect, useRef } from "react";
import type { EditorHandle } from "@/editor";

type Props = {
  editor: React.RefObject<EditorHandle | null>;
  revision: number;
  width: number;
  height: number;
};

/**
 * Blits the core's pixmap onto a 2D canvas.
 *
 * v0.1 redraws the whole surface. The core already accepts a dirty rect, so
 * incremental redraw is a change here and in the wasm binding, not a change to
 * the renderer's interface.
 *
 * Note the inline width/height: these are device pixels coming from the core,
 * not a design token. Canvas geometry never goes through Tailwind.
 */
export function Canvas({ editor, revision, width, height }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const core = editor.current;
    if (!canvas || !core) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const bytes = core.render();
    const image = new ImageData(new Uint8ClampedArray(bytes), width, height);
    ctx.putImageData(image, 0, 0);
  }, [editor, revision, width, height]);

  return (
    <div className="bg-canvas-backdrop flex flex-1 items-center justify-center overflow-auto p-4">
      <canvas
        ref={canvasRef}
        width={width}
        height={height}
        className="rounded-sm bg-white shadow-md"
      />
    </div>
  );
}
