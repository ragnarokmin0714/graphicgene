import { useEffect, useRef } from "react";
import type { EditorHandle } from "@/editor";

type Props = {
  editor: React.RefObject<EditorHandle | null>;
  revision: number;
};

/**
 * Paints the core's pixels onto a 2D canvas covering the viewport.
 *
 * The backing store is in device pixels and the element is sized in CSS
 * pixels so the two meet exactly — one canvas pixel per screen pixel, which
 * is what keeps edges sharp on HiDPI screens. Both numbers come from the
 * core (they are geometry, not design tokens), so they are inline styles.
 *
 * Each revision asks the core to bring its pixels up to date and repaints
 * only what it reports: after a pan, the canvas shifts its own content and
 * puts back the strips that appeared; after an edit, the damaged area. The
 * pixels are read in place from wasm memory. A canvas not yet painted at its
 * current size gets everything once, since a new or resized canvas element
 * starts blank whatever the core last drew.
 */
export function Canvas({ editor, revision }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const image = useRef<ImageData | null>(null);
  const paintedSize = useRef<string | null>(null);
  const core = editor.current;
  const [width, height, dpr] = core ? [core.width, core.height, core.dpr] : [1, 1, 1];

  useEffect(() => {
    const canvas = canvasRef.current;
    const core = editor.current;
    if (!canvas || !core) return;
    // Opaque: every frame starts from the backdrop, and an opaque canvas
    // composites faster.
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) return;

    const { shift, rects } = core.render();
    const pixels = core.pixels();
    if (image.current?.data !== pixels) {
      image.current = new ImageData(pixels, core.width, core.height);
    }
    const size = `${core.width}x${core.height}`;
    if (paintedSize.current !== size) {
      ctx.putImageData(image.current, 0, 0);
      paintedSize.current = size;
      return;
    }
    if (shift[0] !== 0 || shift[1] !== 0) {
      // Drawing a canvas onto itself copies what it showed before the draw.
      ctx.drawImage(canvas, shift[0], shift[1]);
    }
    for (const [x, y, w, h] of rects) {
      ctx.putImageData(image.current, 0, 0, x, y, w, h);
    }
  }, [editor, revision]);

  return (
    <canvas
      ref={canvasRef}
      width={width}
      height={height}
      className="absolute top-0 left-0 block"
      style={{ width: width / dpr, height: height / dpr }}
    />
  );
}
