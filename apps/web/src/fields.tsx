import { cn } from "cn";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { isRgba, parseHex, toHex } from "@/color";
import type { Mixed, Rgba } from "@/editor";

/**
 * The look every panel field shares: a quiet well that shows a ring on
 * hover and a strong one while focused. Height is the `control` density
 * token.
 */
export const FIELD =
  "h-control bg-muted/70 hover:ring-border focus-within:ring-primary! flex min-w-0 items-center rounded-md ring-1 ring-transparent ring-inset transition-shadow";

const INPUT = "h-full min-w-0 flex-1 bg-transparent tabular-nums outline-none";

/**
 * Text typed into a field, kept apart from the value the core reports until
 * it is applied: on Enter, on blur, or on a press anywhere else. Escape
 * throws it away. While nothing has been typed the field shows `shown`,
 * live.
 */
function useDraft(shown: string, apply: (text: string) => void) {
  const [draft, setDraft] = useState<string | null>(null);
  const draftRef = useRef<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const applyRef = useRef(apply);
  useLayoutEffect(() => {
    applyRef.current = apply;
  });

  const discard = () => {
    draftRef.current = null;
    setDraft(null);
  };
  const flush = () => {
    const text = draftRef.current;
    if (text === null) return;
    discard();
    applyRef.current(text);
  };

  // A press elsewhere — on the canvas, on a layer row — can change the
  // selection before this field blurs. Apply what was typed first, to the
  // nodes it was typed for.
  const typing = draft !== null;
  useEffect(() => {
    if (!typing) return;
    const onPress = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || !inputRef.current?.contains(event.target)) flush();
    };
    window.addEventListener("pointerdown", onPress, true);
    return () => window.removeEventListener("pointerdown", onPress, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [typing]);

  const inputProps = {
    ref: inputRef,
    value: draft ?? shown,
    spellCheck: false,
    autoComplete: "off",
    onChange: (event: React.ChangeEvent<HTMLInputElement>) => {
      draftRef.current = event.target.value;
      setDraft(event.target.value);
    },
    // Select everything on the way in, so typing replaces the value. A
    // mouse press would put the caret back, so it focuses by hand.
    onMouseDown: (event: React.MouseEvent<HTMLInputElement>) => {
      if (document.activeElement === event.currentTarget) return;
      event.preventDefault();
      event.currentTarget.focus();
    },
    onFocus: (event: React.FocusEvent<HTMLInputElement>) => event.currentTarget.select(),
    onBlur: flush,
    onKeyDown: (event: React.KeyboardEvent<HTMLInputElement>) => {
      if (event.key === "Enter") {
        flush();
        event.currentTarget.blur();
      } else if (event.key === "Escape") {
        event.preventDefault();
        discard();
        event.currentTarget.blur();
      }
    },
  };
  return { inputProps, draft, discard };
}

function round(value: number, precision: number): number {
  const scale = 10 ** precision;
  return Math.round(value * scale) / scale;
}

function format(value: number, precision: number): string {
  const rounded = round(value, precision);
  return String(Object.is(rounded, -0) ? 0 : rounded);
}

function parseNumber(text: string, unit: string | undefined): number | null {
  const bare = (unit ? text.replace(unit, "") : text).replace(",", ".").trim();
  if (bare === "") return null;
  const value = Number(bare);
  return Number.isFinite(value) ? value : null;
}

type NumberFieldProps = {
  /** What the field is, for screen readers. */
  name: string;
  /** Shown at the left, where dragging scrubs the value. */
  label: React.ReactNode;
  value: number | Mixed;
  unit?: string;
  /** Change per pixel scrubbed and per arrow press; Shift makes it 10×. */
  step?: number;
  min?: number;
  max?: number;
  /** Decimal places kept. */
  precision?: number;
  /** A scrub in progress: each value it passes through. */
  onPreview: (value: number) => void;
  /** The scrub ended: record it. */
  onCommit: () => void;
  /** The scrub was abandoned: put back what was there. */
  onCancel: () => void;
  /** A typed or stepped value, recorded at once. */
  onSet: (value: number) => void;
  className?: string;
};

type Scrub = {
  element: HTMLElement;
  pointer: number;
  x: number;
  start: number;
  last: number;
  moved: boolean;
};

/**
 * A number the user can type, step with the arrow keys, or scrub by
 * dragging its label sideways — Figma's inspector fields. A scrub previews
 * on every move and is one undo step on release; Escape during it puts the
 * value back. A value the selection does not share shows as "Mixed", and
 * can be typed over but not scrubbed or stepped.
 */
export function NumberField({
  name,
  label,
  value,
  unit,
  step = 1,
  min = -Infinity,
  max = Infinity,
  precision = 2,
  onPreview,
  onCommit,
  onCancel,
  onSet,
  className,
}: NumberFieldProps) {
  const scrub = useRef<Scrub | null>(null);
  // While scrubbing, the field shows what it sent, not what the core reads
  // back: several nodes' shared box reads 0° again however far it turned.
  const [scrubbed, setScrubbed] = useState<number | null>(null);
  const mixed = value === "mixed";
  const clamp = (v: number) => Math.min(max, Math.max(min, round(v, precision)));

  const shown = scrubbed !== null ? format(scrubbed, precision) : mixed ? "" : format(value, precision);
  const { inputProps, draft, discard } = useDraft(shown, (text) => {
    const typed = parseNumber(text, unit);
    if (typed !== null) onSet(clamp(typed));
  });

  const endScrub = (commit: boolean) => {
    const current = scrub.current;
    if (!current) return;
    scrub.current = null;
    setScrubbed(null);
    if (current.element.hasPointerCapture?.(current.pointer)) {
      current.element.releasePointerCapture(current.pointer);
    }
    if (commit) onCommit();
    else onCancel();
  };
  const endScrubRef = useRef(endScrub);
  useLayoutEffect(() => {
    endScrubRef.current = endScrub;
  });

  // Escape mid-scrub puts the value back. Handled here, before the app's
  // Escape (which would deselect), and marked as handled so it does not
  // run as well.
  const scrubbing = scrubbed !== null;
  useEffect(() => {
    if (!scrubbing) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      endScrubRef.current(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [scrubbing]);

  const stepBy = (direction: number, shift: boolean) => {
    const base = draft !== null ? parseNumber(draft, unit) : mixed ? null : value;
    if (base === null) return;
    discard();
    onSet(clamp(base + direction * step * (shift ? 10 : 1)));
  };

  return (
    // Not a <label>: the click that ends a scrub would focus the input.
    <div className={cn(FIELD, "group/field", className)}>
      <span
        aria-hidden="true"
        className={cn(
          "text-muted-foreground text-label grid h-full w-6 shrink-0 touch-none place-items-center [&_svg]:size-3",
          mixed ? "cursor-default" : "group-hover/field:text-foreground cursor-ew-resize",
        )}
        onPointerDown={(event) => {
          if (event.button !== 0 || mixed) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          scrub.current = {
            element: event.currentTarget,
            pointer: event.pointerId,
            x: event.clientX,
            start: value,
            last: value,
            moved: false,
          };
          setScrubbed(value);
        }}
        onPointerMove={(event) => {
          const current = scrub.current;
          if (!current) return;
          const dx = event.clientX - current.x;
          if (!current.moved && Math.abs(dx) < 2) return;
          current.moved = true;
          const next = clamp(current.start + Math.round(dx) * step * (event.shiftKey ? 10 : 1));
          if (next === current.last) return;
          current.last = next;
          setScrubbed(next);
          onPreview(next);
        }}
        onPointerUp={(event) => {
          const current = scrub.current;
          if (!current) return;
          endScrub(true);
          // A click on the label, rather than a drag, goes to the input.
          if (!current.moved) {
            event.currentTarget.parentElement?.querySelector("input")?.focus();
          }
        }}
        onPointerCancel={() => endScrub(false)}
        onLostPointerCapture={() => endScrub(false)}
      >
        {label}
      </span>
      <input
        {...inputProps}
        aria-label={name}
        inputMode="decimal"
        placeholder={mixed ? "Mixed" : undefined}
        className={cn(INPUT, "placeholder:text-muted-foreground", !unit && "pr-2")}
        onKeyDown={(event) => {
          if (event.key === "ArrowUp" || event.key === "ArrowDown") {
            event.preventDefault();
            stepBy(event.key === "ArrowUp" ? 1 : -1, event.shiftKey);
            return;
          }
          inputProps.onKeyDown(event);
        }}
      />
      {unit && !mixed && (
        <span aria-hidden="true" className="text-muted-foreground pr-2 pl-0.5">
          {unit}
        </span>
      )}
    </div>
  );
}

/**
 * A colour's six hex digits, typed. Three digits and a leading "#" are
 * accepted too; anything else puts the old value back. `leading` sits in
 * the same well, before the digits: the swatch.
 */
export function HexField({
  name,
  color,
  leading,
  onSet,
  className,
}: {
  name: string;
  color: Rgba | Mixed;
  leading?: React.ReactNode;
  onSet: (rgb: [number, number, number]) => void;
  className?: string;
}) {
  const { inputProps } = useDraft(isRgba(color) ? toHex(color) : "", (text) => {
    const rgb = parseHex(text);
    if (rgb) onSet(rgb);
  });
  return (
    <div className={cn(FIELD, "gap-2 pl-1.5", className)}>
      {leading}
      <input
        {...inputProps}
        aria-label={name}
        maxLength={7}
        placeholder={isRgba(color) ? undefined : "Mixed"}
        className={cn(INPUT, "placeholder:text-muted-foreground pr-2 uppercase")}
      />
    </div>
  );
}
