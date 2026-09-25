import * as React from "react"
import { cn } from "cn"

/**
 * A keycap. Colours derive from `currentColor`, so the same cap reads on a
 * dark tooltip and on a light panel without a variant.
 */
function Kbd({ className, ...props }: React.ComponentProps<"kbd">) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(
        "inline-flex h-4 min-w-4 items-center justify-center rounded-[3px] bg-current/12 px-1 font-sans text-[10px] leading-none font-medium tabular-nums opacity-80",
        className
      )}
      {...props}
    />
  )
}

function KbdGroup({ keys }: { keys: readonly string[] }) {
  return (
    <span data-slot="kbd-group" className="inline-flex items-center gap-0.5">
      {keys.map((key) => (
        <Kbd key={key}>{key}</Kbd>
      ))}
    </span>
  )
}

export { Kbd, KbdGroup }
