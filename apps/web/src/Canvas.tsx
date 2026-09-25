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
    // Bottom padding leaves room for the floating tool dock.
    <div className="absolute inset-0 flex overflow-auto p-10 pb-20">
      <figure className="m-auto flex flex-col gap-1.5">
        <figcaption className="text-label text-muted-foreground flex justify-between px-px">
          <span className="font-medium">Artboard</span>
          <span className="tabular-nums">
            {width} × {height}
          </span>
        </figcaption>
        <canvas
          ref={canvasRef}
          width={width}
          height={height}
          className="shadow-float block bg-white ring-1 ring-black/5"
        />
      </figure>
    </div>
  );
}
