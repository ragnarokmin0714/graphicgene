import { Eye, EyeOff, Folder, Lock, Square } from "lucide-react";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import type { EditorHandle, LayerRow } from "@/editor";

type Props = {
  editor: React.RefObject<EditorHandle | null>;
  revision: number;
  selected: string | null;
  onSelect: (id: string) => void;
};

/**
 * Reads the layer tree from the core on every revision and keeps no copy.
 *
 * Rows are `h-row` (28px) — the density token, not an ad-hoc height — so that
 * every future list in the app lines up without anyone re-deciding.
 */
export function LayerPanel({ editor, revision, selected, onSelect }: Props) {
  const core = editor.current;
  // `revision` drives the re-read; referenced so the intent is visible.
  void revision;
  const layers: LayerRow[] = core ? core.layers() : [];

  return (
    <aside className="bg-card flex w-panel shrink-0 flex-col border-l">
      <h2 className="text-label text-muted-foreground px-2 py-1.5 font-medium tracking-wider uppercase">
        Layers
      </h2>
      <ul className="flex-1 overflow-y-auto px-1 pb-1">
        {layers.map((layer) => (
          <ContextMenu key={layer.id}>
            <ContextMenuTrigger asChild>
              <li
                onClick={() => onSelect(layer.id)}
                data-selected={layer.id === selected || undefined}
                className="h-row hover:bg-accent data-selected:bg-primary data-selected:text-primary-foreground flex cursor-default items-center gap-1.5 rounded-sm px-1.5"
              >
                {layer.kind === "group" ? (
                  <Folder className="size-3 shrink-0 opacity-70" />
                ) : (
                  <Square className="size-3 shrink-0 opacity-70" />
                )}
                <span className={layer.visible ? "truncate" : "truncate opacity-45"}>
                  {layer.name || "(unnamed)"}
                </span>
                <span className="ml-auto flex items-center gap-1 opacity-60">
                  {layer.locked && <Lock className="size-3" />}
                  {layer.visible ? <Eye className="size-3" /> : <EyeOff className="size-3" />}
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
    </aside>
  );
}
