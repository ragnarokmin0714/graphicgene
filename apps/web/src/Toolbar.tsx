import { Circle, FolderOpen, Redo2, Save, Square, Undo2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

type Action = {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
};

function ToolButton({ label, icon, onClick, disabled }: Action) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button size="icon-xs" onClick={onClick} disabled={disabled} aria-label={label}>
          {icon}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="bottom">{label}</TooltipContent>
    </Tooltip>
  );
}

type Props = {
  onAddRect: () => void;
  onAddEllipse: () => void;
  onUndo: () => void;
  onRedo: () => void;
  onSave: () => void;
  onLoad: () => void;
  canUndo: boolean;
  canRedo: boolean;
};

export function Toolbar(props: Props) {
  return (
    <header className="bg-card flex h-8 shrink-0 items-center gap-1 border-b px-2">
      <span className="text-label mr-1 font-semibold tracking-wide">graphicgene</span>
      <Separator orientation="vertical" className="mx-1 !h-4" />

      <ToolButton label="Rectangle" icon={<Square />} onClick={props.onAddRect} />
      <ToolButton label="Ellipse" icon={<Circle />} onClick={props.onAddEllipse} />
      <Separator orientation="vertical" className="mx-1 !h-4" />

      <ToolButton
        label="Undo"
        icon={<Undo2 />}
        onClick={props.onUndo}
        disabled={!props.canUndo}
      />
      <ToolButton
        label="Redo"
        icon={<Redo2 />}
        onClick={props.onRedo}
        disabled={!props.canRedo}
      />

      <div className="ml-auto flex items-center gap-1">
        <ToolButton label="Save" icon={<Save />} onClick={props.onSave} />
        <ToolButton label="Load" icon={<FolderOpen />} onClick={props.onLoad} />
      </div>
    </header>
  );
}
