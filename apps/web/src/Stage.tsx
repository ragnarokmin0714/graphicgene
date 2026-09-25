import { useEffect, useRef, useState } from "react";
import { Canvas } from "@/Canvas";
import type { EditorHandle, Frame, Overlay, Point, Rgba } from "@/editor";
import { HANDLE_SIZE, handleAt, handlesOf } from "@/handles";
import type { Tool } from "@/ToolDock";

type Props = {
  editor: React.RefObject<EditorHandle | null>;
  revision: number;
  run: <T>(fn: (editor: EditorHandle) => T) => T | undefined;
  width: number;
  height: number;
  tool: Tool;
  /** Fill for the next shape drawn. */
  nextFill: () => Rgba;
  /** Called after a shape tool finishes drawing, to hand back to Select. */
  onShapeDrawn: () => void;
};

type PointerState = { at: Point; shift: boolean; alt: boolean };

/**
 * The artboard: the rendered pixmap, the selection overlay on top, and the
 * pointer handling that turns presses and drags into core gestures.
 *
 * All geometry — hit-testing, the frame, outlines, what a drag does — comes
 * from the core. This component only converts pointer positions into
 * document space, picks which gesture a press starts, and draws what
 * `overlay()` reports.
 */
export function Stage({ editor, revision, run, width, height, tool, nextFill, onShapeDrawn }: Props) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const lastPointer = useRef<PointerState | null>(null);
  const [cursor, setCursor] = useState("default");
  // Hover only changes the overlay, not the document, so it gets its own
  // tick instead of bumping the document revision and re-rendering pixels.
  const [, setHoverTick] = useState(0);

  const core = editor.current;
  const overlay = core?.overlay() ?? null;

  // Pressing or releasing Shift/Alt mid-drag re-applies the constraint
  // without waiting for the pointer to move. Not a shortcut, so it does not
  // go through useShortcuts.
  useEffect(() => {
    const onModifier = (event: KeyboardEvent) => {
      const last = lastPointer.current;
      if (!dragging.current || !last) return;
      if (event.key !== "Shift" && event.key !== "Alt") return;
      // Stops Alt from focusing the browser's menu bar on Windows.
      event.preventDefault();
      lastPointer.current = { at: last.at, shift: event.shiftKey, alt: event.altKey };
      run((ed) => ed.updateGesture(last.at[0], last.at[1], event.shiftKey, event.altKey));
    };
    window.addEventListener("keydown", onModifier);
    window.addEventListener("keyup", onModifier);
    return () => {
      window.removeEventListener("keydown", onModifier);
      window.removeEventListener("keyup", onModifier);
    };
  }, [run]);

  /** Client coordinates to document space. Robust to the surface being CSS-scaled. */
  const toDocument = (event: { clientX: number; clientY: number }): Point => {
    const rect = surfaceRef.current?.getBoundingClientRect();
    if (!rect) return [0, 0];
    return [
      ((event.clientX - rect.left) * width) / rect.width,
      ((event.clientY - rect.top) * height) / rect.height,
    ];
  };

  const cursorAt = (p: Point): string => {
    if (tool !== "select") return "crosshair";
    const target = overlay?.frame ? handleAt(overlay.frame, p) : null;
    return target?.cursor ?? "default";
  };

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !core) return;
    // The backdrop clears the selection on its own presses; not this one.
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    const p = toDocument(event);
    const { shiftKey: shift, altKey: alt } = event;
    dragging.current = true;
    lastPointer.current = { at: p, shift, alt };

    if (tool !== "select") {
      run((ed) => ed.beginCreate(tool, p[0], p[1], nextFill()));
      return;
    }
    const target = overlay?.frame ? handleAt(overlay.frame, p) : null;
    run((ed) => {
      if (target?.kind === "scale") {
        ed.beginScale(target.u, target.v, p[0], p[1]);
      } else if (target?.kind === "rotate") {
        ed.beginRotate(p[0], p[1]);
      } else {
        const outcome = ed.selectAt(p[0], p[1], shift);
        if (outcome === "drag") ed.beginMove(p[0], p[1]);
        else if (outcome === "miss") ed.beginMarquee(p[0], p[1], shift);
      }
      ed.clearHover();
    });
    if (target) setCursor(target.cursor);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const p = toDocument(event);
    if (dragging.current) {
      lastPointer.current = { at: p, shift: event.shiftKey, alt: event.altKey };
      run((ed) => ed.updateGesture(p[0], p[1], event.shiftKey, event.altKey));
      return;
    }
    if (!core) return;
    if (tool === "select" && core.hover(p[0], p[1])) setHoverTick((t) => t + 1);
    setCursor(cursorAt(p));
  };

  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    dragging.current = false;
    const created = run((ed) => ed.endGesture());
    if (created) onShapeDrawn();
    setCursor(cursorAt(toDocument(event)));
  };

  // A drag the browser takes away (a system gesture, a lost capture) is
  // abandoned rather than committed half-way.
  const onPointerAbort = () => {
    if (!dragging.current) return;
    dragging.current = false;
    run((ed) => ed.cancelGesture());
  };

  const onPointerLeave = () => {
    if (!dragging.current && core?.clearHover()) setHoverTick((t) => t + 1);
  };

  const onBackdropPointerDown = (event: React.PointerEvent) => {
    if (event.button === 0 && tool === "select" && !event.shiftKey) {
      run((ed) => ed.clearSelection());
    }
  };

  return (
    // Bottom padding leaves room for the floating tool dock.
    <div
      className="absolute inset-0 flex overflow-auto p-10 pb-20"
      onPointerDown={onBackdropPointerDown}
    >
      <figure className="m-auto flex flex-col gap-1.5">
        <figcaption className="text-label text-muted-foreground flex justify-between px-px">
          <span className="font-medium">Artboard</span>
          <span className="tabular-nums">
            {width} × {height}
          </span>
        </figcaption>
        <div
          ref={surfaceRef}
          className="shadow-float relative touch-none ring-1 ring-black/5"
          style={{ width, height, cursor }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerAbort}
          onLostPointerCapture={onPointerAbort}
          onPointerLeave={onPointerLeave}
        >
          <Canvas editor={editor} revision={revision} width={width} height={height} />
          {overlay && <SelectionOverlay overlay={overlay} width={width} height={height} />}
        </div>
      </figure>
    </div>
  );
}

/**
 * Everything drawn over the artwork. Positions come straight from the core's
 * overlay, so they are inline SVG attributes, never Tailwind classes; the
 * classes here only pick colours.
 */
function SelectionOverlay({
  overlay,
  width,
  height,
}: {
  overlay: Overlay;
  width: number;
  height: number;
}) {
  const { frame, outlines, hover, marquee } = overlay;
  return (
    <>
      <svg
        className="pointer-events-none absolute inset-0 overflow-visible"
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        aria-hidden="true"
      >
        {hover && <path d={hover} fill="none" strokeWidth={1.5} className="stroke-primary" />}
        {/* One selected node is outlined by its frame; several get their own
            outlines so it is clear what the shared frame contains. */}
        {outlines.length > 1 &&
          outlines.map((d, i) => (
            <path key={i} d={d} fill="none" strokeWidth={1} className="stroke-primary/70" />
          ))}
        {frame && (
          <polygon
            points={frame.corners.map((c) => c.join(",")).join(" ")}
            fill="none"
            strokeWidth={1}
            className="stroke-primary"
          />
        )}
        {frame &&
          handlesOf(frame).map((handle) => (
            <rect
              key={`${handle.u},${handle.v}`}
              x={handle.at[0] - HANDLE_SIZE / 2}
              y={handle.at[1] - HANDLE_SIZE / 2}
              width={HANDLE_SIZE}
              height={HANDLE_SIZE}
              rx={1.5}
              strokeWidth={1}
              className="stroke-primary fill-white"
            />
          ))}
        {marquee && (
          <rect
            x={marquee[0]}
            y={marquee[1]}
            width={marquee[2] - marquee[0]}
            height={marquee[3] - marquee[1]}
            strokeWidth={1}
            className="fill-primary/8 stroke-primary"
          />
        )}
      </svg>
      {frame && <SizeLabel frame={frame} />}
    </>
  );
}

/** "W × H" under the frame, as in Figma. */
function SizeLabel({ frame }: { frame: Frame }) {
  const bottom = Math.max(...frame.corners.map((c) => c[1]));
  const centerX = frame.corners.reduce((sum, c) => sum + c[0], 0) / 4;
  const round = (n: number) => Math.round(n * 10) / 10;
  return (
    <div
      className="bg-primary text-primary-foreground text-label pointer-events-none absolute rounded px-1.5 py-0.5 font-medium whitespace-nowrap tabular-nums"
      style={{ left: centerX, top: bottom + 8, transform: "translateX(-50%)" }}
    >
      {round(frame.width)} × {round(frame.height)}
    </div>
  );
}
