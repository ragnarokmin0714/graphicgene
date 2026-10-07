import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { resolveBackdrop } from "@/backdrop";
import { Canvas } from "@/Canvas";
import { EYEDROPPER_CURSOR, PEN_CURSOR } from "@/cursors";
import type {
  EditorHandle,
  Frame,
  HandleLine,
  Keys,
  Overlay,
  PathOverlay,
  PenOverlay,
  Point,
  Rgba,
  TextOverlay,
  Tool,
} from "@/editor";
import { FALLBACK } from "@/fonts";
import { HANDLE_SIZE, handleAt, handlesOf } from "@/handles";

type Props = {
  editor: React.RefObject<EditorHandle | null>;
  revision: number;
  run: <T>(fn: (editor: EditorHandle) => T) => T | undefined;
  /** From the core; the cursor depends on it. */
  tool: Tool;
  /** Colour for the next shape or pen path. */
  nextFill: () => Rgba;
};

type PointerState = { at: Point } & Keys;

/** The modifiers the core takes from a pointer or key event; ⌘ counts as Ctrl. */
function keysOf(event: { shiftKey: boolean; altKey: boolean; ctrlKey: boolean; metaKey: boolean }): Keys {
  return { shift: event.shiftKey, alt: event.altKey, ctrl: event.ctrlKey || event.metaKey };
}

/** Whether a press went to the core, or pans the view — the one thing the page keeps. */
type DragKind = "core" | "pan";

/** How long panning must pause before shifted pixels are redrawn exactly. */
const SETTLE_DELAY = 150;
/** Ctrl+wheel and pinch zoom: the zoom is multiplied by e^(-deltaY × this). */
const WHEEL_ZOOM = 0.0015;

/** Pointer position in screen pixels: CSS pixels from the viewport's corner. */
function toScreen(element: HTMLElement | null, event: { clientX: number; clientY: number }): Point {
  const rect = element?.getBoundingClientRect();
  return rect ? [event.clientX - rect.left, event.clientY - rect.top] : [0, 0];
}

function isTextField(target: EventTarget | null): boolean {
  return (
    target instanceof Element && !!target.closest("input, textarea, select, [contenteditable]")
  );
}

/**
 * The canvas: the whole area between the chrome, showing the artboard and
 * everything around it through the view. It turns pointer, wheel and key
 * input into core calls, and draws what `overlay()` reports.
 *
 * Everything it hands the core is in screen pixels — pointer positions and
 * pick tolerances alike — and the core maps them through the zoom and pan.
 * All geometry comes back from the core: hit-testing, frames, outlines,
 * anchors, where the artboard is. What a press does is the core's rule too
 * (`pointerDown`); this component pans the view, finds the selection handle
 * under a press — handles are screen-sized — and picks the cursor.
 */
export function Stage({ editor, revision, run, tool, nextFill }: Props) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const dragging = useRef<DragKind | null>(null);
  const lastPointer = useRef<PointerState | null>(null);
  const spaceHeld = useRef(false);
  const settleTimer = useRef<number | undefined>(undefined);
  const [cursor, setCursor] = useState("default");
  // Hover only changes the overlay, not the document, so it gets its own
  // tick instead of bumping the document revision.
  const [, setHoverTick] = useState(0);

  const core = editor.current;
  const overlay = core?.overlay() ?? null;
  const mode = overlay?.mode ?? null;

  /** Once panning stops, have shifted pixels redrawn exactly. */
  const scheduleSettle = () => {
    window.clearTimeout(settleTimer.current);
    settleTimer.current = window.setTimeout(() => run((ed) => ed.settle()), SETTLE_DELAY);
  };
  const scheduleSettleRef = useRef(scheduleSettle);
  useEffect(() => {
    scheduleSettleRef.current = scheduleSettle;
  });
  useEffect(() => () => window.clearTimeout(settleTimer.current), []);

  // The viewport's size in device pixels — exact where the browser reports
  // them, so the canvas maps one-to-one onto the screen — and the pixel
  // ratio, which changes when the window moves to another screen. Re-run
  // once the core exists, so a size measured before it loaded is not lost.
  useEffect(() => {
    const element = viewportRef.current;
    if (!element || !core) return;
    const measure = (entry?: ResizeObserverEntry) => {
      const dpr = window.devicePixelRatio || 1;
      const box = entry?.devicePixelContentBoxSize?.[0];
      const rect = entry?.contentRect ?? element.getBoundingClientRect();
      const width = box ? box.inlineSize : Math.round(rect.width * dpr);
      const height = box ? box.blockSize : Math.round(rect.height * dpr);
      if (width > 0 && height > 0) run((ed) => ed.setViewport(width, height, dpr));
    };
    const observer = new ResizeObserver(([entry]) => measure(entry));
    try {
      observer.observe(element, { box: "device-pixel-content-box" });
    } catch {
      observer.observe(element);
    }
    let ratio: MediaQueryList | null = null;
    const onRatio = () => {
      measure();
      watchRatio();
    };
    const watchRatio = () => {
      ratio?.removeEventListener("change", onRatio);
      ratio = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      ratio.addEventListener("change", onRatio);
    };
    watchRatio();
    return () => {
      observer.disconnect();
      ratio?.removeEventListener("change", onRatio);
    };
  }, [core, run]);

  // The backdrop follows the theme: the core paints it, so it has to be told.
  useEffect(() => {
    if (!core) return;
    const apply = () => {
      const [r, g, b] = resolveBackdrop();
      run((ed) => ed.setBackdrop(r, g, b));
    };
    apply();
    const observer = new MutationObserver(apply);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  }, [core, run]);

  // Wheel: scroll pans, Ctrl/⌘+wheel and trackpad pinch (which arrives as
  // Ctrl+wheel) zoom about the pointer. Coalesced to one core call per
  // animation frame — a trackpad fires several events per frame, and each
  // zoom is a full redraw. Registered by hand because React's wheel
  // listener is passive and could not stop the page zooming along.
  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    let frame = 0;
    let pan: Point = [0, 0];
    let zoom = 1;
    let anchor: Point = [0, 0];
    const flush = () => {
      frame = 0;
      const [dx, dy] = pan;
      const factor = zoom;
      pan = [0, 0];
      zoom = 1;
      run((ed) => {
        if (dx !== 0 || dy !== 0) ed.panBy(dx, dy);
        if (factor !== 1) ed.zoomBy(factor, anchor[0], anchor[1]);
      });
      if (dx !== 0 || dy !== 0) scheduleSettleRef.current();
    };
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? element.clientHeight : 1;
      if (event.ctrlKey || event.metaKey) {
        zoom *= Math.exp(-event.deltaY * unit * WHEEL_ZOOM);
        anchor = toScreen(element, event);
      } else {
        let [dx, dy] = [event.deltaX * unit, event.deltaY * unit];
        // A plain mouse wheel only scrolls vertically; Shift turns it sideways.
        if (event.shiftKey && dx === 0) [dx, dy] = [dy, 0];
        pan = [pan[0] - dx, pan[1] - dy];
      }
      if (!frame) frame = requestAnimationFrame(flush);
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      element.removeEventListener("wheel", onWheel);
      cancelAnimationFrame(frame);
    };
  }, [run]);

  // Space held: drag to pan, as in every design tool. Not a shortcut — it
  // changes what a drag does — so it does not go through useShortcuts.
  useEffect(() => {
    const onSpace = (event: KeyboardEvent) => {
      if (event.key !== " " || isTextField(event.target)) return;
      // No page scroll, and no "clicking" whatever button has focus.
      event.preventDefault();
      const held = event.type === "keydown";
      if (held === spaceHeld.current) return;
      spaceHeld.current = held;
      if (dragging.current !== "pan") setCursor(held ? "grab" : "default");
    };
    window.addEventListener("keydown", onSpace);
    window.addEventListener("keyup", onSpace);
    return () => {
      window.removeEventListener("keydown", onSpace);
      window.removeEventListener("keyup", onSpace);
    };
  }, []);

  /** Feed a drag position to whatever the press started. */
  const applyDrag = ({ at, ...keys }: PointerState) => {
    run((ed) => ed.pointerMove(at, keys));
  };

  // Pressing or releasing Shift/Alt mid-drag re-applies the constraint
  // without waiting for the pointer to move.
  const applyDragRef = useRef(applyDrag);
  useEffect(() => {
    applyDragRef.current = applyDrag;
  });
  useEffect(() => {
    const onModifier = (event: KeyboardEvent) => {
      const kind = dragging.current;
      const last = lastPointer.current;
      if (!kind || kind === "pan" || !last) return;
      if (!["Shift", "Alt", "Control", "Meta"].includes(event.key)) return;
      // Stops Alt from focusing the browser's menu bar on Windows.
      event.preventDefault();
      lastPointer.current = { at: last.at, ...keysOf(event) };
      applyDragRef.current(lastPointer.current);
    };
    window.addEventListener("keydown", onModifier);
    window.addEventListener("keyup", onModifier);
    return () => {
      window.removeEventListener("keydown", onModifier);
      window.removeEventListener("keyup", onModifier);
    };
  }, []);

  const cursorAt = (p: Point): string => {
    if (spaceHeld.current || tool === "hand") return "grab";
    if (tool === "pen") return PEN_CURSOR;
    if (tool === "eyedropper") return EYEDROPPER_CURSOR;
    if (tool === "direct") return "default";
    if (tool !== "select") return "crosshair";
    if (mode === "path") return "default";
    const target = overlay?.frame && !overlay.locked ? handleAt(overlay.frame, p) : null;
    return target?.cursor ?? "default";
  };

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!core) return;
    // The hand tool's presses are panning too: the core never sees them.
    const pans = event.button === 1 || (event.button === 0 && (spaceHeld.current || tool === "hand"));
    if (event.button !== 0 && !pans) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    const p = toScreen(viewportRef.current, event);
    const keys = keysOf(event);
    lastPointer.current = { at: p, ...keys };

    if (pans) {
      dragging.current = "pan";
      setCursor("grabbing");
      return;
    }
    dragging.current = "core";
    const target =
      tool === "select" && mode === null && overlay?.frame && !overlay.locked
        ? handleAt(overlay.frame, p)
        : null;
    run((ed) => ed.pointerDown(p, keys, target, nextFill()));
    if (target) setCursor(target.cursor);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const p = toScreen(viewportRef.current, event);
    const kind = dragging.current;
    if (kind === "pan") {
      const last = lastPointer.current?.at ?? p;
      lastPointer.current = { at: p, ...keysOf(event) };
      run((ed) => ed.panBy(p[0] - last[0], p[1] - last[1]));
      scheduleSettle();
      return;
    }
    if (kind) {
      lastPointer.current = { at: p, ...keysOf(event) };
      applyDrag(lastPointer.current);
      return;
    }
    if (!core) return;
    // Unpressed, a move only changes the overlay — a hover outline, the
    // pen's preview — so it gets its own tick, not a document revision.
    if (core.pointerMove(p, keysOf(event))) setHoverTick((t) => t + 1);
    setCursor(cursorAt(p));
  };

  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    const kind = dragging.current;
    if (!kind) return;
    dragging.current = null;
    if (kind === "core") run((ed) => ed.pointerUp());
    setCursor(cursorAt(toScreen(viewportRef.current, event)));
  };

  // A drag the browser takes away (a system gesture, a lost capture) is
  // abandoned rather than committed half-way.
  const onPointerAbort = () => {
    const kind = dragging.current;
    if (!kind) return;
    dragging.current = null;
    if (kind === "core") run((ed) => ed.pointerCancel());
  };

  const onPointerLeave = () => {
    if (!dragging.current && core?.clearHover()) setHoverTick((t) => t + 1);
  };

  // What a double-click means — edit a path's points, toggle one — is the
  // core's rule; only a double-click while panning is not one.
  const onDoubleClick = (event: React.MouseEvent) => {
    if (spaceHeld.current) return;
    const p = toScreen(viewportRef.current, event);
    run((ed) => ed.doubleClick(p));
  };

  return (
    <div
      ref={viewportRef}
      data-viewport
      data-view={core ? `${core.zoom} ${core.pan[0]} ${core.pan[1]}` : undefined}
      className="absolute inset-0 touch-none overflow-hidden"
      style={{ cursor }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerAbort}
      onLostPointerCapture={onPointerAbort}
      onPointerLeave={onPointerLeave}
      onDoubleClick={onDoubleClick}
      // The middle button's default is autoscroll; here it pans.
      onMouseDown={(event) => event.button === 1 && event.preventDefault()}
    >
      <Canvas editor={editor} revision={revision} />
      {overlay && <OverlayLayer overlay={overlay} />}
      {overlay?.text && <TextField key={overlay.text.id} text={overlay.text} run={run} />}
    </div>
  );
}

/**
 * Where text is typed: a browser text field over the text box, because that
 * is where input methods for Chinese and every other language live.
 *
 * The field shows only the caret and the selection — its own text is
 * transparent. What you see is the core's layout, drawn on the canvas from
 * what the field holds, sent on every change; the field is set in the same
 * font, size and line height so its caret sits where the core draws. It
 * does not scroll, and has room to spare on the side the text grows
 * towards, so a keystroke never shifts it before the core catches up.
 */
function TextField({ text, run }: { text: TextOverlay; run: Props["run"] }) {
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const element = field.current;
    if (!element) return;
    element.focus();
    element.setSelectionRange(element.value.length, element.value.length);
  }, []);
  useLayoutEffect(() => {
    const element = field.current;
    if (element) element.scrollLeft = element.scrollTop = 0;
  });

  const line = text.size * text.lineHeight;
  const slack = text.size * 2;
  const shift = text.align === "left" ? 0 : text.align === "center" ? -slack / 2 : -slack;
  return (
    <textarea
      ref={field}
      aria-label="Text"
      defaultValue={text.content}
      spellCheck={false}
      autoComplete="off"
      className="caret-primary selection:bg-primary/25 absolute top-0 left-0 m-0 resize-none overflow-hidden border-0 bg-transparent p-0 whitespace-pre text-transparent outline-none"
      // Geometry from the core, so inline: the text box's place on screen.
      style={{
        width: text.width + slack,
        height: text.height + line,
        transform: `matrix(${text.matrix.join(",")}) translateX(${shift}px)`,
        transformOrigin: "0 0",
        fontFamily: `"${text.family}", "${FALLBACK}"`,
        fontSize: text.size,
        lineHeight: `${line}px`,
        textAlign: text.align,
        fontKerning: "normal",
        // The core draws no ligatures, so the field's caret must not expect them.
        fontVariantLigatures: "none",
      }}
      // Presses in the field place the caret; they are not canvas presses.
      onPointerDown={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onInput={(event) => {
        const content = event.currentTarget.value;
        run((ed) => ed.previewText(content));
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          run((ed) => ed.escape());
        }
      }}
    />
  );
}

/**
 * Everything drawn over the artwork, in screen pixels. Positions come
 * straight from the core's overlay, so they are inline attributes and
 * styles, never Tailwind classes; the classes here only pick colours.
 */
function OverlayLayer({ overlay }: { overlay: Overlay }) {
  const { frame, outlines, hover, marquee } = overlay;
  return (
    <>
      <ArtboardChrome artboard={overlay.artboard} />
      <svg
        className="pointer-events-none absolute inset-0 overflow-visible"
        width="100%"
        height="100%"
        aria-hidden="true"
      >
        {hover && <path d={hover} fill="none" strokeWidth={1.5} className="stroke-primary" />}
        {/* One selected node is outlined by its frame; several get their own
            outlines so it is clear what the shared frame contains. */}
        {outlines.length > 1 &&
          outlines.map((d, i) => (
            <path key={i} d={d} fill="none" strokeWidth={1} className="stroke-primary/70" />
          ))}
        {/* A locked selection shows where it is, dashed and without
            handles: the canvas cannot move it. */}
        {frame && (
          <polygon
            points={frame.corners.map((c) => c.join(",")).join(" ")}
            fill="none"
            strokeWidth={1}
            strokeDasharray={overlay.locked ? "4 3" : undefined}
            className="stroke-primary"
          />
        )}
        {frame &&
          !overlay.locked &&
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
        {overlay.guides.map(([x0, y0, x1, y1], i) => (
          <line key={i} x1={x0} y1={y0} x2={x1} y2={y1} strokeWidth={1} className="stroke-rose-500" />
        ))}
        {overlay.pen && <PenLayer pen={overlay.pen} />}
        {overlay.path && <PathLayer path={overlay.path} />}
      </svg>
      {frame && <SizeLabel frame={frame} />}
    </>
  );
}

/**
 * The artboard's name and size above its top-left corner, and the shadow
 * that lifts it off the backdrop. The page itself is painted by the core.
 */
function ArtboardChrome({ artboard }: { artboard: Overlay["artboard"] }) {
  const [x0, y0, x1, y1] = artboard.rect;
  return (
    <>
      <div
        className="shadow-float pointer-events-none absolute ring-1 ring-black/5"
        style={{ left: x0, top: y0, width: x1 - x0, height: y1 - y0 }}
      />
      <div
        className="text-label text-muted-foreground pointer-events-none absolute flex justify-between gap-3 whitespace-nowrap"
        style={{ left: x0, top: y0 - 18, minWidth: x1 - x0 }}
      >
        <span className="font-medium">Artboard</span>
        <span className="tabular-nums">
          {artboard.width} × {artboard.height}
        </span>
      </div>
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

/** "W × H" under the frame, as in Figma — in document units, whatever the zoom. */
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
