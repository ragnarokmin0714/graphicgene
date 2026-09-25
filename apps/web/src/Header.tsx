import { FileDown, FolderOpen, ImageDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { KbdGroup } from "@/components/ui/kbd";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { IconButton } from "@/IconButton";
import { Logo } from "@/Logo";
import { MOD, SHIFT } from "@/shortcuts";
import { ThemeMenu } from "@/ThemeMenu";

type Props = {
  onOpen: () => void;
  onDownload: () => void;
  onExport: () => void;
};

export function Header({ onOpen, onDownload, onExport }: Props) {
  return (
    <header className="bg-card h-bar flex shrink-0 items-center gap-1 border-b px-3">
      <Logo className="size-5" />
      <span className="ml-1.5 text-[13px] font-semibold tracking-tight">graphicgene</span>
      <span className="bg-muted text-muted-foreground text-label ml-2 rounded px-1.5 py-px font-medium">
        v0.1 preview
      </span>

      <div className="ml-auto flex items-center gap-0.5">
        <IconButton
          label="Open project file"
          icon={<FolderOpen />}
          onClick={onOpen}
          shortcut={[MOD, "O"]}
        />
        <IconButton label="Download project file" icon={<FileDown />} onClick={onDownload} />
        <Separator orientation="vertical" className="mx-1.5 !h-4" />
        {/* The one primary action in the chrome: getting the artwork out. */}
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="default" onClick={onExport}>
              <ImageDown />
              Export SVG
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom" sideOffset={6}>
            Download the artwork as SVG
            <KbdGroup keys={[MOD, SHIFT, "E"]} />
          </TooltipContent>
        </Tooltip>
        <Separator orientation="vertical" className="mx-1.5 !h-4" />
        <ThemeMenu />
      </div>
    </header>
  );
}
