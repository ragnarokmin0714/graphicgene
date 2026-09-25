import { useEffect, useRef, useState } from "react";
import { Canvas } from "@/Canvas";
import { PEN_CURSOR } from "@/cursors";
import type {
  EditorHandle,
  Frame,
  HandleLine,
  Overlay,
  PathOverlay,
  PenOverlay,
  Point,
  Rgba,
} from "@/editor";
import { HANDLE_SIZE, PICK_RADIUS, handleAt, handlesOf } from "@/handles";
import type { Tool } from "@/ToolDock";

type Props = {
  editor: React.RefObject<EditorHandle | null>;
  revision: number;
  run: <T>(fn: (editor: EditorHandle) => T) => T | undefined;
  width: number;
  height: number;
  tool: Tool;
  /** Colour for the next shape or pen path. */
  nextFill: () => Rgba;
  /** Called after a shape or pen path is finished, to hand back to Select. */
  onShapeDrawn: () => void;
};

type PointerState = { at: Point; shift: boolean; alt: boolean };

/** Which core API a press started talking to; its moves and release follow. */
type DragKind = "pen" | "path" | "gesture";

/** Pick distance in document units. Zoom is fixed at 100% in v0.1. */
const PICK = PICK_RADIUS;

/**
 * The artboard: the rendered pixmap, the overlay on top, and the pointer
 * handling that turns presses and drags into core calls.
 *
 * All geometry — hit-testing, frames, outlines, anchors, what a drag does —
 * comes from the core. This component converts pointer positions into
 * document space, picks which core API a press goes to, and draws what
 * `overlay()` reports.
 */
export function Stage({ editor, revision, run, width, height, tool, nextFill, onShapeDrawn }: Props) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const dragging = useRef<DragKind | null>(null);
  // What the last press was, so the double-click that ends a pen path is not
  // then taken as "edit this path".
  const lastPress = useRef<DragKind | null>(null);
  const lastPointer = useRef<PointerState | null>(null);
  const [cursor, setCursor] = useState("default");
  // Hover only changes the overlay, not the document, so it gets its own
  // tick instead of bumping the document revision and re-rendering pixels.
  const [, setHoverTick] = useState(0);

  const core = editor.current;
  const overlay = core?.overlay() ?? null;
  const mode = overlay?.mode ?? null;

  /** Feed a drag position to whichever core API the press started. */
  const applyDrag = (kind: DragKind, { at, shift, alt }: PointerState) => {
    run((ed) => {
      if (kind === "pen") ed.penDrag(at[0], at[1], shift);
      else if (kind === "path") ed.pathDrag(at[0], at[1], shift, alt);
      else ed.updateGesture(at[0], at[1], shift, alt);
    });
  };

  // Pressing or releasing Shift/Alt mid-drag re-applies the constraint
  // without waiting for the pointer to move. Not a shortcut, so it does not
  // go through useShortcuts.
  const applyDragRef = useRef(applyDrag);
  useEffect(() => {
    applyDragRef.current = applyDrag;
  });
  useEffect(() => {
    const onModifier = (event: KeyboardEvent) => {
      const kind = dragging.current;
      const last = lastPointer.current;
      if (!kind || !last) return;
      if (event.key !== "Shift" && event.key !== "Alt") return;
      // Stops Alt from focusing the browser's menu bar on Windows.
      event.preventDefault();
      lastPointer.current = { at: last.at, shift: event.shiftKey, alt: event.altKey };
      applyDragRef.current(kind, lastPointer.current);
    };
    window.addEventListener("keydown", onModifier);
    window.addEventListener("keyup", onModifier);
    return () => {
      window.removeEventListener("keydown", onModifier);
      window.removeEventListener("keyup", onModifier);
    };
  }, []);

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
    if (tool === "pen") return PEN_CURSOR;
    if (tool !== "select") return "crosshair";
    if (mode === "path") return "default";
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
    lastPointer.current = { at: p, shift, alt };

    if (tool === "pen") {
      dragging.current = lastPress.current = "pen";
      run((ed) => ed.penPress(p[0], p[1], shift, PICK, nextFill()));
      return;
    }
    if (mode === "path") {
      dragging.current = lastPress.current = "path";
      run((ed) => ed.pathPress(p[0], p[1], PICK, shift));
      return;
    }
    dragging.current = lastPress.current = "gesture";
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
    const kind = dragging.current;
    if (kind) {
      lastPointer.current = { at: p, shift: event.shiftKey, alt: event.altKey };
      applyDrag(kind, lastPointer.current);
      return;
    }
    if (!core) return;
    if (tool === "pen") {
      if (core.penHover(p[0], p[1], PICK)) setHoverTick((t) => t + 1);
    } else if (tool === "select" && mode === null && core.hover(p[0], p[1])) {
      setHoverTick((t) => t + 1);
    }
    setCursor(cursorAt(p));
  };

  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    const kind = dragging.current;
    if (!kind) return;
    dragging.current = null;
    if (kind === "pen") {
      if (run((ed) => ed.penRelease())) onShapeDrawn();
    } else if (kind === "path") {
      run((ed) => ed.pathRelease());
    } else if (run((ed) => ed.endGesture())) {
      onShapeDrawn();
    }
    setCursor(cursorAt(toDocument(event)));
  };

  // A drag the browser takes away (a system gesture, a lost capture) is
  // abandoned rather than committed half-way. A pen press keeps its anchor.
  const onPointerAbort = () => {
    const kind = dragging.current;
    if (!kind) return;
    dragging.current = null;
    if (kind === "path") run((ed) => ed.pathCancelDrag());
    else if (kind === "gesture") run((ed) => ed.cancelGesture());
  };

  const onPointerLeave = () => {
    if (!dragging.current && core?.clearHover()) setHoverTick((t) => t + 1);
  };

  // Double-click a path to edit its points; while editing, double-click a
  // point to toggle it between corner and curve, or empty space to stop.
  const onDoubleClick = (event: React.MouseEvent) => {
    if (tool !== "select" || lastPress.current === "pen") return;
    const p = toDocument(event);
    if (mode === "path") run((ed) => ed.pathDoubleClick(p[0], p[1], PICK));
    else run((ed) => ed.beginPathEdit());
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
          onDoubleClick={onDoubleClick}
        >
          <Canvas editor={editor} revision={revision} width={width} height={height} />
          {overlay && <OverlayLayer overlay={overlay} width={width} height={height} />}
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
function OverlayLayer({ overlay, width, height }: { overlay: Overlay; width: number; height: number }) {
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
        {overlay.pen && <PenLayer pen={overlay.pen} />}
        {overlay.path && <PathLayer path={overlay.path} />}
      </svg>
      {frame && <SizeLabel frame={frame} />}
    </>
  );
}

/** Anchor squares and handle dots, sized in screen px like frame handles. */
const ANCHOR_SIZE = 7;
const HANDLE_DOT = 3.5;

function PenLayer({ pen }: { pen: PenOverlay }) {
  const [first] = pen.anchors;
  return (
    <>
      {pen.preview && (
        <path d={pen.preview} fill="none" strokeWidth={1} className="stroke-primary" />
      )}
      <HandleLines handles={pen.handles} />
      {pen.anchors.map((at, i) => (
        <AnchorMark key={i} at={at} selected={i === pen.anchors.length - 1} />
      ))}
      {/* The ring that says "click here to close". */}
      {pen.closable && first && (
        <circle cx={first[0]} cy={first[1]} r={7} fill="none" strokeWidth={1.5} className="stroke-primary" />
      )}
    </>
  );
}

function PathLayer({ path }: { path: PathOverlay }) {
  return (
    <>
      <path d={path.outline} fill="none" strokeWidth={1} className="stroke-primary" />
      <HandleLines handles={path.handles} />
      {path.anchors.map((anchor, i) => (
        <AnchorMark key={i} at={anchor.at} selected={anchor.selected} />
      ))}
    </>
  );
}

function HandleLines({ handles }: { handles: HandleLine[] }) {
  return handles.map(([ax, ay, hx, hy], i) => (
    <g key={i}>
      <line x1={ax} y1={ay} x2={hx} y2={hy} strokeWidth={1} className="stroke-primary/70" />
      <circle cx={hx} cy={hy} r={HANDLE_DOT} strokeWidth={1} className="stroke-primary fill-white" />
    </g>
  ));
}

function AnchorMark({ at, selected }: { at: Point; selected: boolean }) {
  return (
    <rect
      x={at[0] - ANCHOR_SIZE / 2}
      y={at[1] - ANCHOR_SIZE / 2}
      width={ANCHOR_SIZE}
      height={ANCHOR_SIZE}
      strokeWidth={1}
      data-selected={selected || undefined}
      className="stroke-primary data-selected:fill-primary fill-white"
    />
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
