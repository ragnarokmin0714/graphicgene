import { Button } from "@/components/ui/button";
import { KbdGroup } from "@/components/ui/kbd";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

type Props = {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  shortcut?: readonly string[];
  disabled?: boolean;
  /** Set for toggles such as tools; renders as pressed when true. */
  active?: boolean;
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
  active,
  size = "icon-sm",
  side = "bottom",
}: Props) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {/* The span keeps the tooltip working while the button is disabled. */}
        <span className="inline-flex">
          <Button
            size={size}
            onClick={onClick}
            disabled={disabled}
            aria-label={label}
            aria-pressed={active}
            className="aria-pressed:bg-primary aria-pressed:text-primary-foreground aria-pressed:hover:bg-primary/90 aria-pressed:hover:text-primary-foreground"
          >
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
