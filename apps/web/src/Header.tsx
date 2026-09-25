import { FolderOpen, Save } from "lucide-react";
import { Separator } from "@/components/ui/separator";
import { IconButton } from "@/IconButton";
import { Logo } from "@/Logo";
import { MOD } from "@/shortcuts";
import { ThemeMenu } from "@/ThemeMenu";

type Props = {
  onSave: () => void;
  onLoad: () => void;
};

export function Header({ onSave, onLoad }: Props) {
  return (
    <header className="bg-card h-bar flex shrink-0 items-center gap-1 border-b px-3">
      <Logo className="size-5" />
      <span className="ml-1.5 text-[13px] font-semibold tracking-tight">graphicgene</span>
      <span className="bg-muted text-muted-foreground text-label ml-2 rounded px-1.5 py-px font-medium">
        v0.1 preview
      </span>

      <div className="ml-auto flex items-center gap-0.5">
        <IconButton label="Save" icon={<Save />} onClick={onSave} shortcut={[MOD, "S"]} />
        <IconButton label="Load saved project" icon={<FolderOpen />} onClick={onLoad} />
        <Separator orientation="vertical" className="mx-1.5 !h-4" />
        <ThemeMenu />
      </div>
    </header>
  );
}
