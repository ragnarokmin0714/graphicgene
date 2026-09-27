import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { type Hsv, SWATCHES, hsvToRgb, isRgba, rgbToHsv, toCss, toHex } from "@/color";
import type { Mixed, Rgba } from "@/editor";

type Props = {
  /** What it colours, for screen readers: "Fill", "Stroke". */
  name: string;
  color: Rgba | Mixed;
  /** A drag in progress: each colour it passes through. */
  onPreview: (color: Rgba) => void;
  /** The drag ended: record it. */
  onCommit: () => void;
  /** The drag was abandoned: put back what was there. */
  onCancel: () => void;
  /** A preset or a keyboard step, recorded at once. */
  onSet: (color: Rgba) => void;
};

/** Where the picker starts when the selected colours differ: Figma's default fill. */
const MIXED_START: Rgba = [217, 217, 217, 255];
const PRESETS: readonly Rgba[] = [[0, 0, 0, 255], [255, 255, 255, 255], MIXED_START, ...SWATCHES];

function same(a: Rgba | Mixed, b: Rgba | Mixed): boolean {
  return isRgba(a) && isRgba(b) ? a.every((c, i) => c === b[i]) : a === b;
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/**
 * A swatch that opens a colour picker: saturation and value in a square,
 * then hue and alpha strips, and a row of presets.
 *
 * It works in HSV of sRGB bytes and sends bytes; the core converts them to
 * its linear model. A drag previews on every move and is one undo step on
 * release, and Escape during a drag puts the colour back — a second Escape
 * closes the picker, as it closes any popover.
 */
export function ColorPicker({ name, color, onPreview, onCommit, onCancel, onSet }: Props) {
  const start = isRgba(color) ? color : MIXED_START;
  const [hsv, setHsv] = useState<Hsv>(() => rgbToHsv(start));
  const [alpha, setAlpha] = useState(start[3]);

  // Follow the selection's colour, unless it is the one this picker just
  // sent: re-deriving HSV from bytes would lose a grey's hue and make the
  // thumbs jitter as they are dragged.
  const [followed, setFollowed] = useState<Rgba | Mixed>(color);
  if (!same(followed, color)) {
    setFollowed(color);
    if (isRgba(color) && !same(color, [...hsvToRgb(hsv), alpha])) {
      setHsv(rgbToHsv(color));
      setAlpha(color[3]);
    }
  }

  const rgba = (next: Hsv, a = alpha): Rgba => [...hsvToRgb(next), a];
  const preview = (next: Hsv, a = alpha) => {
    setHsv(next);
    setAlpha(a);
    onPreview(rgba(next, a));
  };
  const set = (next: Hsv, a = alpha) => {
    setHsv(next);
    setAlpha(a);
    onSet(rgba(next, a));
  };

  const drag = useDrag(onCommit, onCancel);
  const hue = rgba({ h: hsv.h, s: 1, v: 1 }, 255);
  const opaque = rgba(hsv, 255);

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={`${name} colour`}
          className="bg-checker ring-border focus-visible:ring-ring size-4 shrink-0 overflow-hidden rounded-sm ring-1 outline-none focus-visible:ring-2"
        >
          {isRgba(color) ? (
            <span className="block size-full" style={{ background: toCss(color) }} />
          ) : (
            <span className="bg-mixed block size-full" />
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent side="left" align="start" sideOffset={16} className="flex w-56 flex-col gap-2.5">
        <Area
          name={`${name} saturation and brightness`}
          className="h-36 rounded-sm"
          style={{
            background: `linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, ${toCss(hue)})`,
          }}
          thumb={[hsv.s, 1 - hsv.v]}
          thumbColor={opaque}
          valueText={`saturation ${Math.round(hsv.s * 100)}%, brightness ${Math.round(hsv.v * 100)}%`}
          drag={drag}
          onDrag={(u, v) => preview({ h: hsv.h, s: u, v: 1 - v })}
          onStep={(dx, dy) => set({ h: hsv.h, s: clamp01(hsv.s + dx), v: clamp01(hsv.v - dy) })}
        />
        <Area
          name={`${name} hue`}
          className="h-3 rounded-full"
          style={{
            backgroundImage:
              "linear-gradient(to right, #f00, #ff0 17%, #0f0 33%, #0ff 50%, #00f 67%, #f0f 83%, #f00)",
          }}
          thumb={[hsv.h / 360, 0.5]}
          thumbColor={hue}
          valueText={`${Math.round(hsv.h)}°`}
          drag={drag}
          onDrag={(u) => preview({ ...hsv, h: Math.min(u * 360, 359.9) })}
          onStep={(dx) => set({ ...hsv, h: Math.min(359.9, Math.max(0, hsv.h + dx * 360)) })}
        />
        <Area
          name={`${name} opacity`}
          className="h-3 rounded-full"
          style={{
            backgroundImage: `linear-gradient(to right, transparent, ${toCss(opaque)}), var(--checker)`,
            backgroundSize: "100% 100%, 8px 8px",
          }}
          thumb={[alpha / 255, 0.5]}
          thumbColor={rgba(hsv)}
          valueText={`${Math.round((alpha / 255) * 100)}%`}
          drag={drag}
          onDrag={(u) => preview(hsv, Math.round(u * 255))}
          onStep={(dx) => set(hsv, Math.round(clamp01(alpha / 255 + dx) * 255))}
        />
        <div className="flex flex-wrap gap-1.5 pt-0.5">
          {PRESETS.map((preset) => (
            <button
              key={preset.join()}
              type="button"
              aria-label={`Use #${toHex(preset)}`}
              className="ring-border focus-visible:ring-ring size-5 rounded-sm ring-1 outline-none focus-visible:ring-2"
              style={{ background: toCss(preset) }}
              onClick={() => set(rgbToHsv(preset), preset[3])}
            />
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}

type Drag = ReturnType<typeof useDrag>;

/**
 * One drag across the picker's areas: capture on press, commit on release,
 * cancel when the browser takes the pointer away or on Escape — which is
 * marked as handled, so the popover stays open and the app does not
 * deselect.
 */
function useDrag(onCommit: () => void, onCancel: () => void) {
  const active = useRef<{ element: HTMLElement; pointer: number } | null>(null);
  const [dragging, setDragging] = useState(false);

  const end = (commit: boolean) => {
    const current = active.current;
    if (!current) return;
    active.current = null;
    setDragging(false);
    if (current.element.hasPointerCapture?.(current.pointer)) {
      current.element.releasePointerCapture(current.pointer);
    }
    if (commit) onCommit();
    else onCancel();
  };
  const endRef = useRef(end);
  useLayoutEffect(() => {
    endRef.current = end;
  });

  useEffect(() => {
    if (!dragging) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      endRef.current(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [dragging]);

  return {
    begin: (element: HTMLElement, pointer: number) => {
      element.setPointerCapture(pointer);
      active.current = { element, pointer };
      setDragging(true);
    },
    isActive: (element: HTMLElement) => active.current?.element === element,
    end,
  };
}

/** Keyboard steps: 1% of the range, 10% with Shift. */
const KEY_STEP = 0.01;

/**
 * A draggable area: the saturation/brightness square, or a strip. Reports
 * positions in unit coordinates of its box; arrow keys step by 1% of it.
 */
function Area({
  name,
  className,
  style,
  thumb,
  thumbColor,
  valueText,
  drag,
  onDrag,
  onStep,
}: {
  name: string;
  className: string;
  style: React.CSSProperties;
  /** Unit coordinates of the thumb. */
  thumb: [number, number];
  thumbColor: Rgba;
  valueText: string;
  drag: Drag;
  onDrag: (u: number, v: number) => void;
  onStep: (dx: number, dy: number) => void;
}) {
  const at = (event: React.PointerEvent<HTMLElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const u = rect.width > 0 ? clamp01((event.clientX - rect.left) / rect.width) : 0;
    const v = rect.height > 0 ? clamp01((event.clientY - rect.top) / rect.height) : 0;
    onDrag(u, v);
  };
  return (
    <div
      role="slider"
      tabIndex={0}
      aria-label={name}
      aria-valuetext={valueText}
      className={`ring-border focus-visible:ring-ring relative shrink-0 touch-none ring-1 outline-none ring-inset focus-visible:ring-2 ${className}`}
      style={style}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        drag.begin(event.currentTarget, event.pointerId);
        at(event);
      }}
      onPointerMove={(event) => {
        if (drag.isActive(event.currentTarget)) at(event);
      }}
      onPointerUp={() => drag.end(true)}
      onPointerCancel={() => drag.end(false)}
      onLostPointerCapture={() => drag.end(false)}
      onKeyDown={(event) => {
        const step = KEY_STEP * (event.shiftKey ? 10 : 1);
        const move: Record<string, [number, number]> = {
          ArrowLeft: [-step, 0],
          ArrowRight: [step, 0],
          ArrowUp: [0, -step],
          ArrowDown: [0, step],
        };
        const delta = move[event.key];
        if (!delta) return;
        // Handled: arrows here must not also nudge the selection.
        event.preventDefault();
        onStep(delta[0], delta[1]);
      }}
    >
      {/* Positioned from the value, so an inline style. */}
      <span
        className="pointer-events-none absolute size-3 -translate-1/2 rounded-full shadow-[0_0_0_1.5px_white,0_0_0_2.5px_rgb(0_0_0/0.25)]"
        style={{
          left: `${thumb[0] * 100}%`,
          top: `${thumb[1] * 100}%`,
          background: toCss(thumbColor),
        }}
      />
    </div>
  );
}
