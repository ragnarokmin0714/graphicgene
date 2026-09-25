type Props = {
  width: number;
  height: number;
  layerCount: number;
  notice: string | null;
};

export function StatusBar({ width, height, layerCount, notice }: Props) {
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
      {notice && (
        <span role="status" className="ml-auto">
          {notice}
        </span>
      )}
    </footer>
  );
}
