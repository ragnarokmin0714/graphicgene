import { Eye, EyeOff, Folder, Layers, Lock, LockOpen, Spline, Type } from "lucide-react";
import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Kbd } from "@/components/ui/kbd";
import type { Arrangement, DropPlace, LayerRow } from "@/editor";
import { MOD, SHIFT } from "@/shortcuts";

/** What the panel asks of the core; each is one undo step, except selecting. */
export type LayerActions = {
  /** A row was clicked; `additive` when Shift was held. */
  select: (id: string, additive: boolean) => void;
  rename: (id: string, name: string) => void;
  setVisible: (id: string, visible: boolean) => void;
  setLocked: (id: string, locked: boolean) => void;
  /** Move the selection where it was dropped. */
  move: (target: string, place: DropPlace) => void;
  arrange: (how: Arrangement) => void;
  group: () => void;
  ungroup: () => void;
  toggleVisible: () => void;
  toggleLocked: () => void;
  remove: () => void;
  duplicate: () => void;
};

type Props = {
  /** Read by the caller and cached on the core's `layersVersion`. */
  layers: LayerRow[];
  actions: LayerActions;
};

/** Indent per tree level, in px. Derived from data, so it is an inline style. */
const INDENT = 12;
/** How far a press must travel before it drags the row, in px. */
const DRAG_SLOP = 4;

type Drop = { id: string; place: DropPlace };

/**
 * The layer rows as the core reports them: already in panel order (topmost
 * first, no root), with selection marked — selection lives in the core too.
 *
 * Rows rename on double-click, show and lock from the icons that appear on
 * hover, and drag to reorder: into the upper or lower half of a row to land
 * above or below it, into the middle of a group to go inside. What a drop
 * means is the core's rule (`moveSelection`); the panel only says where.
 *
 * Memoised, and fed rows cached on `layersVersion`, which holds still for
 * a whole drag: the panel is not rebuilt sixty times a second while the
 * canvas moves.
 *
 * Rows are `h-row` (28px) — the density token, not an ad-hoc height — so that
 * every future list in the app lines up without anyone re-deciding.
 */
export const LayerPanel = memo(function LayerPanel({ layers, actions }: Props) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [drop, setDrop] = useState<Drop | null>(null);
  const [dragging, setDragging] = useState(false);
  const list = useRef<HTMLUListElement>(null);
  const drag = useRef<{ id: string; pointer: number; y: number; active: boolean } | null>(null);
  // The click that may follow a drag's release is not a selection. Reset on
  // every press: with the pointer captured, that click can land on the list
  // instead of a row.
  const dragged = useRef(false);

  /** Rows a drop cannot target: the selected ones and everything inside them. */
  const moving = new Set<string>();
  let inside = Infinity;
  for (const layer of layers) {
    if (layer.depth <= inside) inside = Infinity;
    if (layer.selected || inside < layer.depth) {
      moving.add(layer.id);
      if (layer.selected) inside = Math.min(inside, layer.depth);
    }
  }

  const dropAt = (y: number): Drop | null => {
    const rows = [...(list.current?.querySelectorAll<HTMLElement>("li[data-id]") ?? [])];
    if (rows.length === 0) return null;
    for (const row of rows) {
      const rect = row.getBoundingClientRect();
      if (y < rect.top || y >= rect.bottom) continue;
      const layer = layers.find((l) => l.id === row.dataset.id);
      if (!layer || moving.has(layer.id)) return null;
      const f = (y - rect.top) / rect.height;
      if (layer.kind === "group" && f > 0.25 && f < 0.75) return { id: layer.id, place: "inside" };
      return { id: layer.id, place: f < 0.5 ? "above" : "below" };
    }
    // Past either end: the top of the list, or the bottom of the root.
    const first = layers[0];
    let last: LayerRow | undefined;
    for (const layer of layers) if (layer.depth === 0) last = layer;
    if (y < rows[0].getBoundingClientRect().top) {
      return moving.has(first.id) ? null : { id: first.id, place: "above" };
    }
    return last && !moving.has(last.id) ? { id: last.id, place: "below" } : null;
  };

  const endDrag = (commit: boolean) => {
    const current = drag.current;
    drag.current = null;
    setDrop(null);
    setDragging(false);
    if (!current?.active) return;
    dragged.current = true;
    if (list.current?.hasPointerCapture?.(current.pointer)) {
      list.current.releasePointerCapture(current.pointer);
    }
    if (commit && dropRef.current) actions.move(dropRef.current.id, dropRef.current.place);
  };
  const dropRef = useRef(drop);
  const endDragRef = useRef(endDrag);
  useLayoutEffect(() => {
    dropRef.current = drop;
    endDragRef.current = endDrag;
  });

  // Escape drops nothing: the rows stay where they were.
  useEffect(() => {
    if (!dragging) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      endDragRef.current(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [dragging]);

  const canUngroup = layers.some((l) => l.selected && l.kind === "group");

  return (
    <aside className="bg-card w-panel flex shrink-0 flex-col border-r" aria-label="Layers">
      <div className="h-bar flex shrink-0 items-center gap-2 px-3">
        <h2 className="font-semibold">Layers</h2>
        {layers.length > 0 && (
          <span className="bg-muted text-muted-foreground text-label rounded px-1.5 font-medium tabular-nums">
            {layers.length}
          </span>
        )}
      </div>

      {layers.length === 0 ? (
        <EmptyState />
      ) : (
        <ul
          ref={list}
          className="flex-1 overflow-y-auto px-1.5 pb-1.5"
          onPointerMove={(event) => {
            const current = drag.current;
            if (!current) return;
            if (!current.active) {
              if (Math.abs(event.clientY - current.y) < DRAG_SLOP) return;
              current.active = true;
              setDragging(true);
              list.current?.setPointerCapture(current.pointer);
              // A row dragged from outside the selection is dragged alone.
              if (!layers.find((l) => l.id === current.id)?.selected) actions.select(current.id, false);
            }
            setDrop(dropAt(event.clientY));
          }}
          onPointerUp={() => endDrag(true)}
          onPointerCancel={() => endDrag(false)}
          onLostPointerCapture={() => endDrag(false)}
        >
          {layers.map((layer) => (
            <ContextMenu key={layer.id}>
              <ContextMenuTrigger asChild>
                <li
                  data-id={layer.id}
                  data-selected={layer.selected || undefined}
                  data-drop={drop?.id === layer.id ? drop.place : undefined}
                  style={{ paddingLeft: 8 + layer.depth * INDENT }}
                  className="group h-row hover:bg-accent data-selected:bg-primary/12 dark:data-selected:bg-primary/22 data-[drop=inside]:ring-primary data-[drop=inside]:bg-primary/10 relative flex cursor-default items-center gap-2 rounded-md pr-1 transition-colors data-[drop=inside]:ring-1 data-[drop=inside]:ring-inset"
                  onPointerDown={(event) => {
                    dragged.current = false;
                    if (event.button !== 0 || renaming) return;
                    drag.current = { id: layer.id, pointer: event.pointerId, y: event.clientY, active: false };
                  }}
                  onClick={(event) => {
                    if (dragged.current) {
                      dragged.current = false;
                      return;
                    }
                    actions.select(layer.id, event.shiftKey);
                  }}
                  onDoubleClick={() => setRenaming(layer.id)}
                  // Right-clicking a row outside the selection acts on that row.
                  onContextMenu={() => !layer.selected && actions.select(layer.id, false)}
                >
                  {drop?.id === layer.id && drop.place !== "inside" && (
                    <span
                      aria-hidden="true"
                      className="bg-primary pointer-events-none absolute right-1 h-0.5 rounded-full"
                      style={{
                        left: 8 + layer.depth * INDENT,
                        [drop.place === "above" ? "top" : "bottom"]: -1,
                      }}
                    />
                  )}
                  <KindIcon kind={layer.kind} />
                  {renaming === layer.id ? (
                    <RenameInput
                      name={layer.name}
                      onDone={(name) => {
                        setRenaming(null);
                        if (name !== null) actions.rename(layer.id, name);
                      }}
                    />
                  ) : (
                    <span className={layer.visible ? "truncate" : "truncate opacity-45"}>
                      {layer.name || "(unnamed)"}
                    </span>
                  )}
                  <span className="ml-auto flex shrink-0 items-center">
                    <RowToggle
                      label={`${layer.locked ? "Unlock" : "Lock"} ${layer.name}`}
                      on={layer.locked}
                      onToggle={() => actions.setLocked(layer.id, !layer.locked)}
                    >
                      {layer.locked ? <Lock /> : <LockOpen />}
                    </RowToggle>
                    <RowToggle
                      label={`${layer.visible ? "Hide" : "Show"} ${layer.name}`}
                      on={!layer.visible}
                      onToggle={() => actions.setVisible(layer.id, !layer.visible)}
                    >
                      {layer.visible ? <Eye /> : <EyeOff />}
                    </RowToggle>
                  </span>
                </li>
              </ContextMenuTrigger>
              {/* Radix supplies the focus trap, roving tabindex and dismiss
                  behaviour these menus need — the reason shadcn is here.
                  Focus is not handed back to the row, so Rename's input
                  keeps it. */}
              <ContextMenuContent className="min-w-48" onCloseAutoFocus={(event) => event.preventDefault()}>
                <ContextMenuItem onSelect={() => setRenaming(layer.id)}>Rename</ContextMenuItem>
                <ContextMenuItem onSelect={actions.duplicate}>
                  Duplicate
                  <ContextMenuShortcut>{MOD} D</ContextMenuShortcut>
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem onSelect={actions.group}>
                  Group selection
                  <ContextMenuShortcut>{MOD} G</ContextMenuShortcut>
                </ContextMenuItem>
                <ContextMenuItem disabled={!canUngroup} onSelect={actions.ungroup}>
                  Ungroup
                  <ContextMenuShortcut>
                    {MOD} {SHIFT} G
                  </ContextMenuShortcut>
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem onSelect={() => actions.arrange("forward")}>
                  Bring forward
                  <ContextMenuShortcut>{MOD} ]</ContextMenuShortcut>
                </ContextMenuItem>
                <ContextMenuItem onSelect={() => actions.arrange("backward")}>
                  Send backward
                  <ContextMenuShortcut>{MOD} [</ContextMenuShortcut>
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem onSelect={actions.toggleVisible}>
                  {layer.visible ? "Hide" : "Show"}
                  <ContextMenuShortcut>
                    {MOD} {SHIFT} H
                  </ContextMenuShortcut>
                </ContextMenuItem>
                <ContextMenuItem onSelect={actions.toggleLocked}>
                  {layer.locked ? "Unlock" : "Lock"}
                  <ContextMenuShortcut>
                    {MOD} {SHIFT} L
                  </ContextMenuShortcut>
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem variant="destructive" onSelect={actions.remove}>
                  Delete
                  <ContextMenuShortcut>Del</ContextMenuShortcut>
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenu>
          ))}
        </ul>
      )}
    </aside>
  );
});

function KindIcon({ kind }: { kind: LayerRow["kind"] }) {
  const Icon = kind === "group" ? Folder : kind === "text" ? Type : Spline;
  return <Icon className="text-muted-foreground group-data-selected:text-primary size-3.5 shrink-0" />;
}

/**
 * The eye and the lock on a row. They appear on hover, and stay while they
 * are on — a hidden or locked layer — so a list of identical icons is not
 * noise.
 */
function RowToggle({
  label,
  on,
  onToggle,
  children,
}: {
  label: string;
  on: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={on}
      className="text-muted-foreground hover:text-foreground focus-visible:ring-ring grid size-5 place-items-center rounded opacity-0 outline-none group-hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-1 aria-pressed:opacity-100 [&_svg]:size-3"
      // Neither a drag nor a selection starts here.
      onPointerDown={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation();
        onToggle();
      }}
    >
      {children}
    </button>
  );
}

/**
 * The row's name, being edited: Enter or leaving the field keeps it,
 * Escape puts the old one back. The core ignores a blank or unchanged name.
 */
function RenameInput({ name, onDone }: { name: string; onDone: (name: string | null) => void }) {
  const done = useRef(false);
  const finish = (value: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(value);
  };
  return (
    <input
      aria-label="Layer name"
      defaultValue={name}
      autoFocus
      spellCheck={false}
      onFocus={(event) => event.currentTarget.select()}
      className="bg-background ring-primary h-5 min-w-0 flex-1 rounded-sm px-1 ring-1 outline-none"
      onPointerDown={(event) => event.stopPropagation()}
      onClick={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        if (event.key === "Enter") finish(event.currentTarget.value);
        else if (event.key === "Escape") {
          event.preventDefault();
          finish(null);
        }
      }}
      onBlur={(event) => finish(event.currentTarget.value)}
    />
  );
}

function EmptyState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 pb-12 text-center">
      <div className="bg-muted text-muted-foreground mb-1 grid size-9 place-items-center rounded-lg">
        <Layers className="size-4" />
      </div>
      <p className="font-medium">No layers yet</p>
      <p className="text-muted-foreground leading-relaxed">
        Press <Kbd>R</Kbd>, <Kbd>O</Kbd> or <Kbd>P</Kbd>, then draw on the artboard.
      </p>
    </div>
  );
}
