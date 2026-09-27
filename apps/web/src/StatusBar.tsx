import { ChevronUp } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { MOD, SHIFT } from "@/shortcuts";

export type ZoomActions = {
  zoomIn: () => void;
  zoomOut: () => void;
  zoomTo100: () => void;
  zoomToFit: () => void;
};

type Props = {
  /** The artboard, in document units. */
  artboard: { width: number; height: number };
  /** Screen pixels per document unit: 1 is 100%. */
  zoom: number;
  zoomActions: ZoomActions;
  layerCount: number;
  selectionCount: number;
  /** How to use the current tool or mode, when that is not obvious. */
  hint: string | null;
  notice: string | null;
};

export function StatusBar({
  artboard,
  zoom,
  zoomActions,
  layerCount,
  selectionCount,
  hint,
  notice,
}: Props) {
  return (
    <footer className="bg-card text-muted-foreground text-label h-status flex shrink-0 items-center gap-3 border-t px-3 tabular-nums">
      <span>
        {artboard.width} × {artboard.height} px
      </span>
      <span aria-hidden="true" className="bg-border h-3 w-px" />
      <ZoomMenu zoom={zoom} actions={zoomActions} />
      <span aria-hidden="true" className="bg-border h-3 w-px" />
      <span>
        {layerCount} {layerCount === 1 ? "layer" : "layers"}
      </span>
      {selectionCount > 0 && (
        <>
          <span aria-hidden="true" className="bg-border h-3 w-px" />
          <span className="text-primary font-medium">{selectionCount} selected</span>
        </>
      )}
      <span className="ml-auto flex min-w-0 items-center gap-3">
        {hint && <span className="text-foreground/70 truncate">{hint}</span>}
        {notice && (
          <>
            {hint && <span aria-hidden="true" className="bg-border h-3 w-px shrink-0" />}
            <span role="status" className="shrink-0">
              {notice}
            </span>
          </>
        )}
      </span>
    </footer>
  );
}

function ZoomMenu({ zoom, actions }: { zoom: number; actions: ZoomActions }) {
  // Below 10%, a decimal keeps small zooms from all reading "0%" or "1%".
  const percent = zoom < 0.1 ? (zoom * 100).toFixed(1) : String(Math.round(zoom * 100));
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          aria-label={`Zoom ${percent}%`}
          className="text-label text-muted-foreground -mx-1.5 h-5 gap-0.5 px-1.5 font-normal tabular-nums"
        >
          {percent}%
          <ChevronUp className="size-3 opacity-60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" className="min-w-44">
        <DropdownMenuItem onSelect={actions.zoomIn}>
          Zoom in
          <DropdownMenuShortcut>{MOD} +</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={actions.zoomOut}>
          Zoom out
          <DropdownMenuShortcut>{MOD} −</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={actions.zoomTo100}>
          Zoom to 100%
          <DropdownMenuShortcut>{MOD} 0</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={actions.zoomToFit}>
          Zoom to fit
          <DropdownMenuShortcut>{SHIFT} 1</DropdownMenuShortcut>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
