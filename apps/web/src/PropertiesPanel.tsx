import {
  ALargeSmall,
  AlignCenterHorizontal,
  AlignCenterVertical,
  AlignEndHorizontal,
  AlignEndVertical,
  AlignHorizontalDistributeCenter,
  AlignStartHorizontal,
  AlignStartVertical,
  AlignVerticalDistributeCenter,
  Blend,
  ChevronDown,
  Minus,
  Plus,
  RotateCcw,
  SlidersHorizontal,
  TextAlignCenter,
  TextAlignEnd,
  TextAlignStart,
  UnfoldVertical,
} from "lucide-react";
import { memo } from "react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ColorPicker } from "@/ColorPicker";
import { isRgba, toCss } from "@/color";
import type {
  Alignment,
  Axis,
  FillKind,
  GradientFill,
  Mixed,
  Properties,
  PropertyChange,
  Rgba,
  StrokeCap,
  StrokeJoin,
  TextAlign,
} from "@/editor";
import { FIELD, HexField, NumberField } from "@/fields";
import { IconButton } from "@/IconButton";
import { FAMILY_NAMES } from "@/fonts";

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

export type ArrangeActions = {
  onAlign: (how: Alignment) => void;
  onDistribute: (axis: Axis) => void;
};

type Props = PropertyActions &
  ArrangeActions & {
    /** From the core, identity-stable while unchanged; null with nothing selected. */
    properties: Properties | null;
  };

/** The align buttons, in the order design tools lay them out. */
const ALIGN_BUTTONS: { how: Alignment; label: string; icon: React.ReactNode }[] = [
  { how: "left", label: "Align left", icon: <AlignStartVertical /> },
  { how: "center-x", label: "Align horizontal centres", icon: <AlignCenterVertical /> },
  { how: "right", label: "Align right", icon: <AlignEndVertical /> },
  { how: "top", label: "Align top", icon: <AlignStartHorizontal /> },
  { how: "center-y", label: "Align vertical centres", icon: <AlignCenterHorizontal /> },
  { how: "bottom", label: "Align bottom", icon: <AlignEndHorizontal /> },
];

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

function Sections({
  properties: p,
  onPreview,
  onCommit,
  onCancel,
  onSet,
  onAlign,
  onDistribute,
}: Props & { properties: Properties }) {
  /** The four callbacks a field needs, for one property. */
  const field = <T,>(change: (value: T) => PropertyChange) => ({
    onPreview: (value: T) => onPreview(change(value)),
    onCommit,
    onCancel,
    onSet: (value: T) => onSet(change(value)),
  });

  return (
    <div className="flex-1 overflow-y-auto pb-3">
      {/* One layer aligns to the artboard, several to their combined box. */}
      <Section title={p.count > 1 ? "Align" : "Align to artboard"}>
        <div className="flex items-center gap-0.5">
          {ALIGN_BUTTONS.map(({ how, label, icon }) => (
            <IconButton key={how} label={label} icon={icon} onClick={() => onAlign(how)} />
          ))}
          <span className="bg-border mx-1 h-4 w-px" />
          <IconButton
            label="Distribute horizontal spacing"
            icon={<AlignHorizontalDistributeCenter />}
            disabled={p.count < 3}
            onClick={() => onDistribute("horizontal")}
          />
          <IconButton
            label="Distribute vertical spacing"
            icon={<AlignVerticalDistributeCenter />}
            disabled={p.count < 3}
            onClick={() => onDistribute("vertical")}
          />
        </div>
      </Section>

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

      {p.text && (
        <Section title="Text">
          <FamilyMenu family={p.text.family} onSet={(fontFamily) => onSet({ fontFamily })} />
          <div className="grid grid-cols-2 gap-1.5">
            <NumberField
              name="Font size"
              label={<ALargeSmall />}
              value={p.text.size}
              min={1}
              {...field((fontSize: number) => ({ fontSize }))}
            />
            <NumberField
              name="Line height"
              label={<UnfoldVertical />}
              value={p.text.lineHeight}
              min={0.5}
              step={0.05}
              {...field((lineHeight: number) => ({ lineHeight }))}
            />
          </div>
          <Segmented
            label="Text alignment"
            options={TEXT_ALIGNS}
            value={p.text.align}
            onSet={(textAlign) => onSet({ textAlign })}
          />
        </Section>
      )}

      {p.fill !== undefined && (
        <FillSection fill={p.fill} onPreview={onPreview} onCommit={onCommit} onCancel={onCancel} onSet={onSet} />
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
            <>
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
              <div className="flex items-center gap-2">
                <Segmented
                  label="Stroke ends"
                  options={CAPS}
                  value={p.strokeCap ?? "butt"}
                  onSet={(strokeCap) => onSet({ strokeCap })}
                />
                <Segmented
                  label="Stroke corners"
                  options={JOINS}
                  value={p.strokeJoin ?? "miter"}
                  onSet={(strokeJoin) => onSet({ strokeJoin })}
                />
              </div>
              {/* 0 is a solid line: typing a dash or a gap starts dashing. */}
              <div className="grid grid-cols-2 gap-1.5">
                <NumberField
                  name="Dash"
                  label="Dash"
                  value={p.strokeDash ?? 0}
                  min={0}
                  step={1}
                  {...field((strokeDash: number) => ({ strokeDash }))}
                />
                <NumberField
                  name="Gap"
                  label="Gap"
                  value={p.strokeGap ?? 0}
                  min={0}
                  step={1}
                  {...field((strokeGap: number) => ({ strokeGap }))}
                />
              </div>
            </>
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
 * The fill: a colour, a gradient, or none, with a switch between them.
 * Changing the type keeps what it can — a colour fades out into a
 * gradient, a gradient's first colour stays when it is made solid.
 */
function FillSection({
  fill,
  onPreview,
  onCommit,
  onCancel,
  onSet,
}: PropertyActions & { fill: Rgba | GradientFill | null | Mixed }) {
  const action =
    fill === null ? (
      <Button size="icon-xs" aria-label="Add fill" onClick={() => onSet({ fill: NEW_FILL })}>
        <Plus />
      </Button>
    ) : (
      <Button size="icon-xs" aria-label="Remove fill" onClick={() => onSet({ fill: null })}>
        <Minus />
      </Button>
    );
  if (fill === null) return <Section title="Fill" action={action} />;
  const kind: FillKind | Mixed = fill === "mixed" ? "mixed" : isRgba(fill) ? "solid" : fill.kind;
  const change = <T,>(make: (value: T) => PropertyChange) => ({
    onPreview: (value: T) => onPreview(make(value)),
    onCommit,
    onCancel,
    onSet: (value: T) => onSet(make(value)),
  });
  return (
    <Section title="Fill" action={action}>
      <Segmented label="Fill type" options={FILL_KINDS} value={kind} onSet={(fillKind) => onSet({ fillKind })} />
      {fill === "mixed" || isRgba(fill) ? (
        <ColorFields name="Fill" color={fill} {...change((color: Rgba) => ({ fill: color }))} />
      ) : (
        <>
          <div
            aria-hidden
            className="bg-checker h-3 overflow-hidden rounded-sm border"
          >
            <div className="size-full" style={{ background: previewCss(fill) }} />
          </div>
          {fill.kind === "linear" && (
            <div className="grid grid-cols-2 gap-1.5">
              <NumberField
                name="Gradient angle"
                label={<RotateCcw />}
                value={fill.angle}
                unit="°"
                {...change((fillAngle: number) => ({ fillAngle }))}
              />
            </div>
          )}
          {fill.stops.map((stop, index) => (
            // Stops keep their order — an offset stays between its
            // neighbours' — so the index names one for as long as it lives.
            <div key={index} className="flex gap-1.5">
              <HexField
                name={`Stop ${index + 1} hex`}
                color={stop.color}
                className="min-w-0 flex-1"
                leading={
                  <ColorPicker
                    name={`Stop ${index + 1}`}
                    color={stop.color}
                    {...change((color: Rgba) => ({ fillStopColor: { index, color } }))}
                  />
                }
                onSet={(rgb) => onSet({ fillStopColor: { index, color: [...rgb, stop.color[3]] } })}
              />
              <NumberField
                name={`Stop ${index + 1} position`}
                label={<span className="text-muted-foreground">@</span>}
                value={stop.offset * 100}
                unit="%"
                min={0}
                max={100}
                precision={0}
                className="w-[4.5rem]"
                {...change((percent: number) => ({ fillStopOffset: { index, offset: percent / 100 } }))}
              />
              {fill.stops.length > 2 && (
                <Button size="icon-xs" aria-label={`Remove stop ${index + 1}`} onClick={() => onSet({ removeFillStop: index })}>
                  <Minus />
                </Button>
              )}
            </div>
          ))}
          <Button size="xs" className="w-fit" aria-label="Add stop" onClick={() => onSet({ addFillStop: true })}>
            <Plus />
            Add stop
          </Button>
        </>
      )}
    </Section>
  );
}

/** A gradient's stops, left to right, as CSS draws them: the bar over the stop rows. */
function previewCss(gradient: GradientFill): string {
  const stops = gradient.stops.map((stop) => `${toCss(stop.color)} ${stop.offset * 100}%`);
  return `linear-gradient(to right, ${stops.join(", ")})`;
}

/** A colour's swatch with its picker, hex digits and alpha. */
function ColorFields({
  name,
  color,
  onPreview,
  onCommit,
  onCancel,
  onSet,
}: {
  name: string;
  color: Rgba | Mixed;
  onPreview: (color: Rgba) => void;
  onCommit: () => void;
  onCancel: () => void;
  onSet: (color: Rgba) => void;
}) {
  // Alpha is edited in percent; the colour keeps its bytes.
  const alpha = isRgba(color) ? (color[3] / 255) * 100 : "mixed";
  const withAlpha = (percent: number): Rgba | null =>
    isRgba(color) ? [color[0], color[1], color[2], Math.round((percent / 100) * 255)] : null;
  return (
    <div className="flex gap-1.5">
      <HexField
        name={`${name} hex`}
        color={color}
        className="flex-1"
        leading={
          <ColorPicker
            name={name}
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
        name={`${name} alpha`}
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
  );
}

/**
 * A stroke: its colour with a picker, hex digits and alpha, and a button
 * that adds or removes it. `color` is null when there is none.
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
  return (
    <Section title={title} action={action}>
      <ColorFields
        name={title}
        color={color}
        onPreview={onPreview}
        onCommit={onCommit}
        onCancel={onCancel}
        onSet={onSet}
      />
      {children}
    </Section>
  );
}

/**
 * The families on offer. Picking one the fonts for have not arrived yet is
 * fine: the core reports the characters it cannot set, and they are
 * fetched.
 */
function FamilyMenu({ family, onSet }: { family: string | Mixed; onSet: (family: string) => void }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label="Font family"
          className={`${FIELD} hover:bg-muted w-full justify-between px-2 outline-none`}
        >
          <span className={family === "mixed" ? "text-muted-foreground" : ""}>
            {family === "mixed" ? "Mixed" : family || "No font"}
          </span>
          <ChevronDown className="text-muted-foreground size-3" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-(--radix-dropdown-menu-trigger-width)">
        {FAMILY_NAMES.map((name) => (
          <DropdownMenuItem key={name} onSelect={() => onSet(name)} style={{ fontFamily: `"${name}"` }}>
            {name}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

type Option<T> = { value: T; label: string; icon: React.ReactNode };

const TEXT_ALIGNS: readonly Option<TextAlign>[] = [
  { value: "left", label: "Align text left", icon: <TextAlignStart /> },
  { value: "center", label: "Centre text", icon: <TextAlignCenter /> },
  { value: "right", label: "Align text right", icon: <TextAlignEnd /> },
];

const FILL_KINDS: readonly Option<FillKind>[] = [
  { value: "solid", label: "Solid fill", icon: <FillKindIcon kind="solid" /> },
  { value: "linear", label: "Linear gradient", icon: <FillKindIcon kind="linear" /> },
  { value: "radial", label: "Radial gradient", icon: <FillKindIcon kind="radial" /> },
];

const CAPS: readonly Option<StrokeCap>[] = [
  { value: "butt", label: "Flat ends", icon: <CapIcon cap="butt" /> },
  { value: "round", label: "Round ends", icon: <CapIcon cap="round" /> },
  { value: "square", label: "Square ends", icon: <CapIcon cap="square" /> },
];

const JOINS: readonly Option<StrokeJoin>[] = [
  { value: "miter", label: "Sharp corners", icon: <JoinIcon join="miter" /> },
  { value: "round", label: "Round corners", icon: <JoinIcon join="round" /> },
  { value: "bevel", label: "Bevelled corners", icon: <JoinIcon join="bevel" /> },
];

/** A few icon buttons of which one is on, or none for a mixed selection. */
function Segmented<T extends string>({
  label,
  options,
  value,
  onSet,
}: {
  label: string;
  options: readonly Option<T>[];
  value: T | Mixed;
  onSet: (value: T) => void;
}) {
  return (
    <div role="group" aria-label={label} className="bg-muted/70 flex w-fit gap-0.5 rounded-md p-0.5">
      {options.map((option) => (
        <Button
          key={option.value}
          size="icon-xs"
          aria-label={option.label}
          aria-pressed={value === option.value}
          className="aria-pressed:bg-background aria-pressed:shadow-xs size-5"
          onClick={() => onSet(option.value)}
        >
          {option.icon}
        </Button>
      ))}
    </div>
  );
}

/** A square in one tone, in bands, or in rings: solid, linear, radial. */
function FillKindIcon({ kind }: { kind: FillKind }) {
  return (
    <svg viewBox="0 0 12 12" fill="currentColor">
      {kind === "solid" && <rect x="2" y="2" width="8" height="8" rx="1.5" />}
      {kind === "linear" &&
        [1, 0.55, 0.2].map((opacity, i) => (
          <rect key={i} x={2 + (i * 8) / 3} y="2" width={8 / 3} height="8" opacity={opacity} />
        ))}
      {kind === "radial" &&
        [
          [4.5, 0.2],
          [3, 0.55],
          [1.5, 1],
        ].map(([r, opacity]) => <circle key={r} cx="6" cy="6" r={r} opacity={opacity} />)}
    </svg>
  );
}

/** A thick line stopping at a thin mark: the cap draws itself past it, or not. */
function CapIcon({ cap }: { cap: StrokeCap }) {
  return (
    <svg viewBox="0 0 12 12" fill="none" stroke="currentColor">
      <path d="M1 6h6" strokeWidth={4} strokeLinecap={cap} />
      <path d="M7 1.5v9" strokeWidth={0.75} opacity={0.55} />
    </svg>
  );
}

/** A thick corner drawn with the join it stands for. */
function JoinIcon({ join }: { join: StrokeJoin }) {
  return (
    <svg viewBox="0 0 12 12" fill="none" stroke="currentColor">
      <path d="M2.5 11V3.5H11" strokeWidth={3} strokeLinejoin={join} />
    </svg>
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
