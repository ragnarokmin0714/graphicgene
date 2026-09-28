import { Circle, MousePointer2, PenTool, Redo2, Square, Type, Undo2 } from "lucide-react";
import { Separator } from "@/components/ui/separator";
import type { Tool } from "@/editor";
import { IconButton } from "@/IconButton";
import { MOD, SHIFT } from "@/shortcuts";


type Props = {
  tool: Tool;
  onToolChange: (tool: Tool) => void;
  onUndo: () => void;
  onRedo: () => void;
  canUndo: boolean;
  canRedo: boolean;
};

const TOOLS: { tool: Tool; label: string; icon: React.ReactNode; key: string }[] = [
  { tool: "select", label: "Select", icon: <MousePointer2 />, key: "V" },
  { tool: "rect", label: "Rectangle", icon: <Square />, key: "R" },
  { tool: "ellipse", label: "Ellipse", icon: <Circle />, key: "O" },
  { tool: "pen", label: "Pen", icon: <PenTool />, key: "P" },
  { tool: "text", label: "Text", icon: <Type />, key: "T" },
];

/**
 * The floating dock over the canvas. Tools and history live here, next to
 * the work, rather than in the header.
 */
export function ToolDock(props: Props) {
  return (
    <div
      role="toolbar"
      aria-label="Tools"
      className="bg-card/85 shadow-float absolute bottom-4 left-1/2 flex -translate-x-1/2 items-center gap-0.5 rounded-xl border p-1 backdrop-blur-md"
    >
      {TOOLS.map(({ tool, label, icon, key }) => (
        <IconButton
          key={tool}
          size="tool"
          side="top"
          label={label}
          icon={icon}
          onClick={() => props.onToolChange(tool)}
          shortcut={[key]}
          active={props.tool === tool}
        />
      ))}
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
