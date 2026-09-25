import { Circle, Redo2, Square, Undo2 } from "lucide-react";
import { Separator } from "@/components/ui/separator";
import { IconButton } from "@/IconButton";
import { MOD, SHIFT } from "@/shortcuts";

type Props = {
  onAddRect: () => void;
  onAddEllipse: () => void;
  onUndo: () => void;
  onRedo: () => void;
  canUndo: boolean;
  canRedo: boolean;
};

/**
 * The floating dock over the canvas. Shapes and history live here, next to
 * the work, rather than in the header.
 */
export function ToolDock(props: Props) {
  return (
    <div
      role="toolbar"
      aria-label="Tools"
      className="bg-card/85 shadow-float absolute bottom-4 left-1/2 flex -translate-x-1/2 items-center gap-0.5 rounded-xl border p-1 backdrop-blur-md"
    >
      <IconButton
        size="tool"
        side="top"
        label="Rectangle"
        icon={<Square />}
        onClick={props.onAddRect}
        shortcut={["R"]}
      />
      <IconButton
        size="tool"
        side="top"
        label="Ellipse"
        icon={<Circle />}
        onClick={props.onAddEllipse}
        shortcut={["O"]}
      />
      <Separator orientation="vertical" className="mx-1 !h-5" />
      <IconButton
        size="tool"
        side="top"
        label="Undo"
        icon={<Undo2 />}
        onClick={props.onUndo}
        disabled={!props.canUndo}
        shortcut={[MOD, "Z"]}
      />
      <IconButton
        size="tool"
        side="top"
        label="Redo"
        icon={<Redo2 />}
        onClick={props.onRedo}
        disabled={!props.canRedo}
        shortcut={[MOD, SHIFT, "Z"]}
      />
    </div>
  );
}
