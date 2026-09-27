import { useEffect, useRef } from "react";
import type { EditorHandle } from "@/editor";

type Props = {
  editor: React.RefObject<EditorHandle | null>;
  revision: number;
  width: number;
  height: number;
};

/**
 * Paints the core's pixels onto a 2D canvas.
 *
 * Each revision asks the core to bring its pixels up to date, and only the
 * area it reports as changed is put on the canvas — read in place from wasm
 * memory, never copied across the boundary. A canvas that has not been
 * painted at its current size gets everything once: a new or resized canvas
 * element starts blank whatever the core last drew.
 *
 * Note the inline width/height: these are device pixels coming from the core,
 * not a design token. Canvas geometry never goes through Tailwind.
 */
export function Canvas({ editor, revision, width, height }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const image = useRef<ImageData | null>(null);
  const paintedSize = useRef<string | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const core = editor.current;
    if (!canvas || !core) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const [x, y, w, h] = core.render();
    const pixels = core.pixels();
    if (image.current?.data !== pixels) {
      image.current = new ImageData(pixels, core.width, core.height);
    }
    const size = `${core.width}x${core.height}`;
    if (paintedSize.current !== size) {
      ctx.putImageData(image.current, 0, 0);
      paintedSize.current = size;
    } else if (w > 0 && h > 0) {
      ctx.putImageData(image.current, 0, 0, x, y, w, h);
    }
  }, [editor, revision, width, height]);

  return <canvas ref={canvasRef} width={width} height={height} className="block bg-white" />;
}
