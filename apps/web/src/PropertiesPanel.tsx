import { Blend, Minus, Plus, RotateCcw, SlidersHorizontal } from "lucide-react";
import { memo } from "react";
import { Button } from "@/components/ui/button";
import { ColorPicker } from "@/ColorPicker";
import { isRgba } from "@/color";
import type { Mixed, Properties, PropertyChange, Rgba } from "@/editor";
import { HexField, NumberField } from "@/fields";

export type PropertyActions = {
  /** Show a change without recording it: a scrub or a picker drag in progress. */
  onPreview: (change: PropertyChange) => void;
  /** Record what the previews changed, as one undo step. */
  onCommit: () => void;
  /** Drop the previews. */
  onCancel: () => void;
  /** A typed value, a step, a swatch: one undo step at once. */
  onSet: (change: PropertyChange) => void;
};

type Props = PropertyActions & {
  /** From the core, identity-stable while unchanged; null with nothing selected. */
  properties: Properties | null;
};

/** What a new fill or stroke starts as: Figma's grey fill, a black hairline. */
const NEW_FILL: Rgba = [217, 217, 217, 255];
const NEW_STROKE: Rgba = [0, 0, 0, 255];

/**
 * What is selected, as numbers and colours that can be changed: position,
 * size and rotation, opacity, fill and stroke.
 *
 * Every value comes from the core (`properties()`), and every change goes
 * back to it; nothing is kept here. Continuous controls preview and commit
 * once, so a scrub or a picker drag is one undo step, like a canvas drag.
 *
 * Memoised, with an identity-stable `properties`: it re-renders only when a
 * value it shows changes — though during a canvas drag that is every frame.
 */
export const PropertiesPanel = memo(function PropertiesPanel({ properties, ...actions }: Props) {
  return (
    <aside className="bg-card w-panel flex shrink-0 flex-col border-l" aria-label="Properties">
      <div className="h-bar flex shrink-0 items-center gap-2 px-3">
        <h2 className="font-semibold">Properties</h2>
        {properties && properties.count > 1 && (
          <span className="text-muted-foreground ml-auto tabular-nums">
            {properties.count} layers
          </span>
        )}
      </div>
      {properties ? <Sections properties={properties} {...actions} /> : <EmptyState />}
    </aside>
  );
});

function Sections({ properties: p, onPreview, onCommit, onCancel, onSet }: Props & { properties: Properties }) {
  /** The four callbacks a field needs, for one property. */
  const field = <T,>(change: (value: T) => PropertyChange) => ({
    onPreview: (value: T) => onPreview(change(value)),
    onCommit,
    onCancel,
    onSet: (value: T) => onSet(change(value)),
  });

  return (
    <div className="flex-1 overflow-y-auto pb-3">
      <Section title="Position">
        <div className="grid grid-cols-2 gap-1.5">
          <NumberField name="X" label="X" value={p.x} {...field((x: number) => ({ x }))} />
          <NumberField name="Y" label="Y" value={p.y} {...field((y: number) => ({ y }))} />
          <NumberField
            name="Width"
            label="W"
            value={p.width}
            min={0.01}
            {...field((width: number) => ({ width }))}
          />
          <NumberField
            name="Height"
            label="H"
            value={p.height}
            min={0.01}
            {...field((height: number) => ({ height }))}
          />
          <NumberField
            name="Rotation"
            label={<RotateCcw />}
            value={p.rotation}
            unit="°"
            {...field((rotation: number) => ({ rotation }))}
          />
        </div>
      </Section>

      <Section title="Layer">
        <div className="grid grid-cols-2 gap-1.5">
          <NumberField
            name="Opacity"
            label={<Blend />}
            value={p.opacity === "mixed" ? "mixed" : p.opacity * 100}
            unit="%"
            min={0}
            max={100}
            precision={0}
            {...field((percent: number) => ({ opacity: percent / 100 }))}
          />
        </div>
      </Section>

      {p.fill !== undefined && (
        <PaintSection
          title="Fill"
          color={p.fill}
          onAdd={() => onSet({ fill: NEW_FILL })}
          onRemove={() => onSet({ fill: null })}
          {...field((fill: Rgba) => ({ fill }))}
        />
      )}

      {p.stroke !== undefined && (
        <PaintSection
          title="Stroke"
          color={p.stroke}
          onAdd={() => onSet({ strokeColor: NEW_STROKE })}
          onRemove={() => onSet({ stroke: null })}
          {...field((strokeColor: Rgba) => ({ strokeColor }))}
        >
          {p.strokeWidth !== undefined && (
            <div className="grid grid-cols-2 gap-1.5">
              <NumberField
                name="Stroke width"
                label={<StrokeWidthIcon />}
                value={p.strokeWidth}
                min={0}
                step={0.5}
                {...field((strokeWidth: number) => ({ strokeWidth }))}
              />
            </div>
          )}
        </PaintSection>
      )}
    </div>
  );
}

function Section({
  title,
  action,
  children,
}: {
  title: string;
  action?: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <section className="border-t px-3 pt-2 pb-3 first:border-t-0 first:pt-0">
      <div className="flex h-7 items-center">
        <h3 className="text-label text-muted-foreground font-semibold">{title}</h3>
        {action && <div className="-mr-1.5 ml-auto">{action}</div>}
      </div>
      {children && <div className="flex flex-col gap-1.5">{children}</div>}
    </section>
  );
}

/**
 * A fill or a stroke: its colour with a picker, hex digits and alpha, and a
 * button that adds or removes it. `color` is null when there is none.
 */
function PaintSection({
  title,
  color,
  onAdd,
  onRemove,
  onPreview,
  onCommit,
  onCancel,
  onSet,
  children,
}: {
  title: string;
  color: Rgba | null | Mixed;
  onAdd: () => void;
  onRemove: () => void;
  onPreview: (color: Rgba) => void;
  onCommit: () => void;
  onCancel: () => void;
  onSet: (color: Rgba) => void;
  children?: React.ReactNode;
}) {
  const lower = title.toLowerCase();
  const action =
    color === null ? (
      <Button size="icon-xs" aria-label={`Add ${lower}`} onClick={onAdd}>
        <Plus />
      </Button>
    ) : (
      <Button size="icon-xs" aria-label={`Remove ${lower}`} onClick={onRemove}>
        <Minus />
      </Button>
    );
  if (color === null) return <Section title={title} action={action} />;

  // Alpha is edited in percent; the colour keeps its bytes.
  const alpha = isRgba(color) ? (color[3] / 255) * 100 : "mixed";
  const withAlpha = (percent: number): Rgba | null =>
    isRgba(color) ? [color[0], color[1], color[2], Math.round((percent / 100) * 255)] : null;
  return (
    <Section title={title} action={action}>
      <div className="flex gap-1.5">
        <HexField
          name={`${title} hex`}
          color={color}
          className="flex-1"
          leading={
            <ColorPicker
              name={title}
              color={color}
              onPreview={onPreview}
              onCommit={onCommit}
              onCancel={onCancel}
              onSet={onSet}
            />
          }
          onSet={(rgb) => onSet([...rgb, isRgba(color) ? color[3] : 255])}
        />
        <NumberField
          name={`${title} alpha`}
          label={<span className="bg-checker size-3 rounded-[2px]" />}
          value={alpha}
          unit="%"
          min={0}
          max={100}
          precision={0}
          className="w-[4.5rem]"
          onPreview={(percent) => {
            const next = withAlpha(percent);
            if (next) onPreview(next);
          }}
          onCommit={onCommit}
          onCancel={onCancel}
          onSet={(percent) => {
            const next = withAlpha(percent);
            if (next) onSet(next);
          }}
        />
      </div>
      {children}
    </Section>
  );
}

/** Three lines of growing weight: the usual sign for stroke width. */
function StrokeWidthIcon() {
  return (
    <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeLinecap="round">
      <path d="M2 2.5h8" strokeWidth={1} />
      <path d="M2 6h8" strokeWidth={1.6} />
      <path d="M2 9.75h8" strokeWidth={2.3} />
    </svg>
  );
}

function EmptyState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 pb-12 text-center">
      <div className="bg-muted text-muted-foreground mb-1 grid size-9 place-items-center rounded-lg">
        <SlidersHorizontal className="size-4" />
      </div>
      <p className="font-medium">Nothing selected</p>
      <p className="text-muted-foreground leading-relaxed">
        Select a layer to change its position, size and colours.
      </p>
    </div>
  );
}
