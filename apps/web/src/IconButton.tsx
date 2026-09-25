import { Button } from "@/components/ui/button";
import { KbdGroup } from "@/components/ui/kbd";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

type Props = {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  shortcut?: readonly string[];
  disabled?: boolean;
  size?: "icon-sm" | "tool";
  side?: "top" | "bottom";
};

/** An icon-only button whose tooltip names the action and its shortcut. */
export function IconButton({
  label,
  icon,
  onClick,
  shortcut,
  disabled,
  size = "icon-sm",
  side = "bottom",
}: Props) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {/* The span keeps the tooltip working while the button is disabled. */}
        <span className="inline-flex">
          <Button size={size} onClick={onClick} disabled={disabled} aria-label={label}>
            {icon}
          </Button>
        </span>
      </TooltipTrigger>
      <TooltipContent side={side} sideOffset={6}>
        {label}
        {shortcut && <KbdGroup keys={shortcut} />}
      </TooltipContent>
    </Tooltip>
  );
}
