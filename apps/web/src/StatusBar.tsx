type Props = {
  width: number;
  height: number;
  layerCount: number;
  selectionCount: number;
  /** How to use the current tool or mode, when that is not obvious. */
  hint: string | null;
  notice: string | null;
};

export function StatusBar({ width, height, layerCount, selectionCount, hint, notice }: Props) {
  return (
    <footer className="bg-card text-muted-foreground text-label h-status flex shrink-0 items-center gap-3 border-t px-3 tabular-nums">
      <span>
        {width} × {height} px
      </span>
      <span aria-hidden="true" className="bg-border h-3 w-px" />
      <span>100%</span>
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
