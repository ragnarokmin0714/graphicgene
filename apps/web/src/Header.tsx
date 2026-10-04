import { ChevronDown, FileDown, FolderOpen, ImageDown } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { IconButton } from "@/IconButton";
import { Logo } from "@/Logo";
import { MOD, SHIFT } from "@/shortcuts";
import { ThemeMenu } from "@/ThemeMenu";

type Props = {
  /** In the desktop app, files are saved where the user picks; in a browser, downloaded. */
  desktop: boolean;
  onOpen: () => void;
  onDownload: () => void;
  onExportSvg: () => void;
  /** `scale` pixels per document unit; `transparent` leaves out the white page. */
  onExportPng: (scale: number, transparent: boolean) => void;
};

/** PNG scales offered, as in every design tool's export menu. */
const PNG_SCALES = [1, 2, 3];

export function Header({ desktop, onOpen, onDownload, onExportSvg, onExportPng }: Props) {
  // A per-viewer choice, like the theme: not part of the document.
  const [transparent, setTransparent] = useState(false);
  return (
    <header className="bg-card h-bar flex shrink-0 items-center gap-1 border-b px-3">
      <Logo className="size-5" />
      <span className="ml-1.5 text-[13px] font-semibold tracking-tight">graphicgene</span>
      <span className="bg-muted text-muted-foreground text-label ml-2 rounded px-1.5 py-px font-medium tabular-nums">
        v{__APP_VERSION__} preview
      </span>

      <div className="ml-auto flex items-center gap-0.5">
        <IconButton
          label="Open project file"
          icon={<FolderOpen />}
          onClick={onOpen}
          shortcut={[MOD, "O"]}
        />
        <IconButton
          label={desktop ? "Save project file as…" : "Download project file"}
          icon={<FileDown />}
          onClick={onDownload}
        />
        <Separator orientation="vertical" className="mx-1.5 !h-4" />
        {/* The one primary action in the chrome: getting the artwork out. */}
        <DropdownMenu>
          <Tooltip>
            <TooltipTrigger asChild>
              <DropdownMenuTrigger asChild>
                <Button variant="default">
                  <ImageDown />
                  Export
                  <ChevronDown className="-mr-0.5 opacity-70" />
                </Button>
              </DropdownMenuTrigger>
            </TooltipTrigger>
            <TooltipContent side="bottom" sideOffset={6}>
              {desktop ? "Save the artwork as SVG or PNG" : "Download the artwork as SVG or PNG"}
            </TooltipContent>
          </Tooltip>
          <DropdownMenuContent align="end" className="min-w-52">
            <DropdownMenuItem onSelect={onExportSvg}>
              SVG
              <DropdownMenuShortcut>
                {MOD} {SHIFT} E
              </DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            {PNG_SCALES.map((scale) => (
              <DropdownMenuItem key={scale} onSelect={() => onExportPng(scale, transparent)}>
                PNG
                <DropdownMenuShortcut>{scale}×</DropdownMenuShortcut>
              </DropdownMenuItem>
            ))}
            <DropdownMenuCheckboxItem
              checked={transparent}
              onCheckedChange={(checked) => setTransparent(checked === true)}
              // Ticking it keeps the menu open, so a PNG can follow.
              onSelect={(event) => event.preventDefault()}
            >
              Transparent background
            </DropdownMenuCheckboxItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Separator orientation="vertical" className="mx-1.5 !h-4" />
        <ThemeMenu />
      </div>
    </header>
  );
}
