import { EyeOff, Folder, Layers, Lock, Spline } from "lucide-react";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Kbd } from "@/components/ui/kbd";
import type { EditorHandle, LayerRow } from "@/editor";

type Props = {
  editor: React.RefObject<EditorHandle | null>;
  revision: number;
  /** A row was clicked; `additive` when Shift was held. */
  onSelect: (id: string, additive: boolean) => void;
};

/** Indent per tree level, in px. Derived from data, so it is an inline style. */
const INDENT = 12;

/**
 * Reads the layer tree from the core on every revision and keeps no copy.
 * The core already hands rows back in panel order (topmost first, no root),
 * and says which are selected — selection lives in the core too.
 *
 * Rows are `h-row` (28px) — the density token, not an ad-hoc height — so that
 * every future list in the app lines up without anyone re-deciding.
 */
export function LayerPanel({ editor, revision, onSelect }: Props) {
  const core = editor.current;
  // `revision` drives the re-read; referenced so the intent is visible.
  void revision;
  const layers: LayerRow[] = core ? core.layers() : [];

  return (
    <aside className="bg-card w-panel flex shrink-0 flex-col border-l">
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
        <ul className="flex-1 overflow-y-auto px-1.5 pb-1.5">
          {layers.map((layer) => (
            <ContextMenu key={layer.id}>
              <ContextMenuTrigger asChild>
                <li
                  onClick={(event) => onSelect(layer.id, event.shiftKey)}
                  data-selected={layer.selected || undefined}
                  style={{ paddingLeft: 8 + layer.depth * INDENT }}
                  className="group h-row hover:bg-accent data-selected:bg-primary/12 dark:data-selected:bg-primary/22 flex cursor-default items-center gap-2 rounded-md pr-2 transition-colors"
                >
                  {layer.kind === "group" ? (
                    <Folder className="text-muted-foreground group-data-selected:text-primary size-3.5 shrink-0" />
                  ) : (
                    <Spline className="text-muted-foreground group-data-selected:text-primary size-3.5 shrink-0" />
                  )}
                  <span className={layer.visible ? "truncate" : "truncate opacity-45"}>
                    {layer.name || "(unnamed)"}
                  </span>
                  {/* Only non-default state is shown; a row of identical eye
                      icons is noise until they become toggles. */}
                  <span className="text-muted-foreground ml-auto flex items-center gap-1">
                    {layer.locked && <Lock className="size-3" />}
                    {!layer.visible && <EyeOff className="size-3" />}
                  </span>
                </li>
              </ContextMenuTrigger>
              <ContextMenuContent>
                {/* Radix supplies the focus trap, roving tabindex and dismiss
                    behaviour these menus need — the reason shadcn is here. */}
                <ContextMenuItem disabled>Rename</ContextMenuItem>
                <ContextMenuItem disabled>Duplicate</ContextMenuItem>
                <ContextMenuItem disabled variant="destructive">
                  Delete
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenu>
          ))}
        </ul>
      )}
    </aside>
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
