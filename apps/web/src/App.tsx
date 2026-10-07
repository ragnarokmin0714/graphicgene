import { X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { SWATCHES } from "@/color";
import type { EditorMode, Tool } from "@/editor";
import { encodePng } from "@/files";
import { loadBaseFonts, loadMissingFonts } from "@/fonts";
import { Header } from "@/Header";
import { type LayerActions, LayerPanel } from "@/LayerPanel";
import { detectPlatform, type OpenedFile, type Platform } from "@/platform";
import { type ArrangeActions, type PropertyActions, PropertiesPanel } from "@/PropertiesPanel";
import { MOD, type Shortcut, useShortcuts } from "@/shortcuts";
import { Stage } from "@/Stage";
import { StatusBar, type ViewActions } from "@/StatusBar";
import { ToolDock } from "@/ToolDock";
import { useEditor } from "@/useEditor";
import { useKeymap } from "@/useKeymap";

/**
 * The artboard for a brand-new document. After that the size belongs to the
 * document — a loaded file brings its own — so it is read back from the core.
 */
const NEW_ARTBOARD = { width: 800, height: 600 };
const PROJECT_FILE = "graphicgene-project.json";
const SVG_FILE = "graphicgene.svg";
const PNG_FILE = "graphicgene.png";
/** Quiet time after the last edit before autosaving, in ms. */
const AUTOSAVE_DELAY = 800;
/** Arrow-key nudge, and with Shift held, in document units. */
const NUDGE = 1;
const NUDGE_LARGE = 10;

const timeFormat = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });

/** Status-bar guidance for tools and modes whose controls are not visible. */
function hintFor(tool: Tool, mode: EditorMode | null): string | null {
  if (mode === "pen") {
    return `Click the first point to close · Enter or Esc to finish · ${MOD}+Z removes the last point`;
  }
  if (mode === "path") {
    return "Drag points and handles · Click a segment to add a point · Double-click a point for corner/curve · Enter, Esc or a click away to finish";
  }
  if (mode === "text") return "Type · Esc or click outside to finish";
  switch (tool) {
    case "pen":
      return "Click to add a point · Drag to pull out a curve";
    case "rect":
    case "ellipse":
      return "Drag to draw · Shift for equal sides · Alt from the centre";
    case "text":
      return "Click to add text · Click text to type into it";
    default:
      return null;
  }
}

/**
 * The app shell. Every handler sends a command into the core and re-reads
 * what came back; nothing here mirrors document state. Selection lives in the core
 * as well, because it drives transforms.
 *
 * What React does keep is view state: the active tool, and `notice`, a
 * status-bar message about the last save or load.
 *
 * Persistence is app-layer IO: the core hands over JSON, and this component
 * autosaves it after each pause in editing, restores it on the next visit,
 * and moves project files and exports in and out. Where they go is the
 * platform's business (`platform.ts`): IndexedDB and downloads in a
 * browser, files and the system's dialogs in the desktop app.
 */
export function App() {
  const { editor, revision, error, clearError, run, ready } = useEditor(
    NEW_ARTBOARD.width,
    NEW_ARTBOARD.height,
  );
  const [notice, setNotice] = useState<string | null>(null);
  const { keymap, setKeymap } = useKeymap();
  /** Null until known; nothing is read or written before then. */
  const [platform, setPlatform] = useState<Platform | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  /**
   * Autosave stays off until the stored project has been read back — or
   * for good, if it could not be loaded, so an empty document never
   * overwrites a project this build failed to open.
   */
  const restored = useRef(false);
  const lastSaved = useRef<string | null>(null);

  const core = editor.current;
  const artboard = core?.artboard ?? NEW_ARTBOARD;
  const zoom = core?.zoom ?? 1;
  // Rows are re-read only when the core says they may have changed — never
  // during a drag, which is when this component re-renders every frame.
  const layersVersion = core?.layersVersion ?? null;
  const layers = useMemo(
    () => (core && layersVersion !== null ? core.layers() : []),
    [core, layersVersion],
  );
  const layerCount = layers.length;
  const selectionCount = core?.selectionCount ?? 0;
  const mode = core?.mode ?? null;
  // The tool is session state in the core, like the selection: a finished
  // shape hands back to Select there, not here.
  const tool: Tool = core?.tool ?? "select";
  // Read every render, since a canvas drag changes it every frame; the
  // object stays the same while nothing in it changes, so the panel is
  // memoised on it.
  const properties = core?.properties() ?? null;
  const nextFill = () => SWATCHES[layerCount % SWATCHES.length];

  const layerActions: LayerActions = useMemo(
    () => ({
      select: (id, additive) => run((editor) => editor.selectLayer(id, additive)),
      rename: (id, name) => run((editor) => editor.rename(id, name)),
      setVisible: (id, visible) => run((editor) => editor.setVisible(id, visible)),
      setLocked: (id, locked) => run((editor) => editor.setLocked(id, locked)),
      move: (target, place) => run((editor) => editor.moveSelection(target, place)),
      arrange: (how) => run((editor) => editor.arrange(how)),
      group: () => run((editor) => editor.group()),
      ungroup: () => run((editor) => editor.ungroup()),
      toggleVisible: () => run((editor) => editor.toggleVisible()),
      toggleLocked: () => run((editor) => editor.toggleLocked()),
      remove: () => run((editor) => editor.deleteSelection()),
      duplicate: () => run((editor) => editor.duplicate()),
    }),
    [run],
  );

  // Copy, cut and paste arrive as the browser's clipboard events, the one
  // place a page may use the system clipboard without asking. A text field
  // keeps its own; everything else is the document's.
  useEffect(() => {
    const inTextField = (target: EventTarget | null) =>
      target instanceof Element && !!target.closest("input, textarea, select, [contenteditable]");
    const onCopy = (event: ClipboardEvent) => {
      if (inTextField(event.target) || !event.clipboardData) return;
      const text = run((editor) => (event.type === "cut" ? editor.cut() : editor.copy()));
      if (!text) return;
      event.clipboardData.setData("text/plain", text);
      event.preventDefault();
    };
    const onPaste = (event: ClipboardEvent) => {
      if (inTextField(event.target)) return;
      const text = event.clipboardData?.getData("text/plain");
      if (text && run((editor) => editor.paste(text))) event.preventDefault();
    };
    document.addEventListener("copy", onCopy);
    document.addEventListener("cut", onCopy);
    document.addEventListener("paste", onPaste);
    return () => {
      document.removeEventListener("copy", onCopy);
      document.removeEventListener("cut", onCopy);
      document.removeEventListener("paste", onPaste);
    };
  }, [run]);

  const propertyActions: PropertyActions = useMemo(
    () => ({
      onPreview: (change) => run((editor) => editor.previewProperty(change)),
      onCommit: () => run((editor) => editor.commitProperty()),
      onCancel: () => run((editor) => editor.cancelProperty()),
      onSet: (change) => run((editor) => editor.setProperty(change)),
    }),
    [run],
  );

  const arrangeActions: ArrangeActions = useMemo(
    () => ({
      onAlign: (how) => run((editor) => editor.align(how)),
      onDistribute: (axis) => run((editor) => editor.distribute(axis)),
    }),
    [run],
  );

  const undo = () => run((editor) => editor.undo());
  const redo = () => run((editor) => editor.redo());

  useEffect(() => {
    let live = true;
    detectPlatform()
      .then((found) => live && setPlatform(found))
      .catch(() => live && setNotice("Could not start the app's file storage"));
    return () => {
      live = false;
    };
  }, []);

  /** Write the autosave unless unchanged; `force` reports even then. */
  const save = async (force = false) => {
    const core = editor.current;
    if (!core || !platform || !restored.current || core.busy) return;
    const json = core.toJson();
    if (json === lastSaved.current && !force) return;
    try {
      await platform.writeProject(json);
      lastSaved.current = json;
      setNotice(`Saved ${timeFormat.format(new Date())}`);
    } catch {
      setNotice(platform.desktop ? "Could not write the autosave file" : "Could not save in this browser");
    }
  };

  // Fonts: each family's Latin slice once the core is up, then whatever
  // characters the core could not set, each time it lays text out. This
  // runs after the canvas has drawn — child effects first — so it sees the
  // layout that drawing just did.
  useEffect(() => {
    if (!ready || !editor.current) return;
    void loadBaseFonts(editor.current).then(() => run(() => {}));
  }, [ready, run, editor]);
  const glyphsSeen = useRef(0);
  useEffect(() => {
    const core = editor.current;
    if (!core || core.glyphsVersion === glyphsSeen.current) return;
    glyphsSeen.current = core.glyphsVersion;
    void loadMissingFonts(core).then((added) => added && run(() => {}));
  });

  // Restore the last session once the core is up and the storage is known.
  useEffect(() => {
    if (!ready || !platform) return;
    let cancelled = false;
    platform
      .readProject()
      .then((text) => {
        if (cancelled) return;
        if (text) {
          const loaded = run((editor) => {
            editor.loadJson(text);
            return true;
          });
          if (!loaded) {
            setNotice("Autosave paused: the saved project could not be opened");
            return;
          }
          lastSaved.current = text;
          setNotice("Restored your last session");
        }
        restored.current = true;
      })
      .catch(() => {
        if (!cancelled) {
          setNotice(platform.desktop ? "Autosave paused: the autosave file could not be read" : "Autosave is unavailable in this browser");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [ready, run, platform]);

  // Autosave after each pause in editing. A press in progress is skipped;
  // its release bumps the revision and schedules another try.
  useEffect(() => {
    if (!ready) return;
    const timer = setTimeout(() => void save(), AUTOSAVE_DELAY);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revision, ready]);

  // Leaving the tab is the last reliable moment to write.
  useEffect(() => {
    const onHide = () => {
      if (document.visibilityState === "hidden") void save();
    };
    document.addEventListener("visibilitychange", onHide);
    return () => document.removeEventListener("visibilitychange", onHide);
  });

  const loadOpened = ({ name, text }: OpenedFile) => {
    const opened = run((editor) => {
      editor.loadJson(text);
      return true;
    });
    if (opened) setNotice(`Opened ${name}`);
  };

  const openFile = async (file: File) => loadOpened({ name: file.name, text: await file.text() });

  /** The desktop's open dialog, or the page's file input in a browser. */
  const open = () => {
    if (!platform?.openProject) {
      fileInput.current?.click();
      return;
    }
    platform
      .openProject()
      .then((file) => file && loadOpened(file))
      .catch(() => setNotice("Could not open the file"));
  };

  /** A download in a browser; the save dialog on the desktop, which reports where it went. */
  const deliver = (name: string, data: string | Blob, type: string) => {
    if (!platform) return;
    platform
      .saveFile(name, data, type)
      .then((saved) => saved && platform.desktop && setNotice(`Saved ${saved}`))
      .catch(() => setNotice(`Could not save ${name}`));
  };

  const downloadProject = () => {
    const json = run((editor) => editor.toJson());
    if (json !== undefined) deliver(PROJECT_FILE, json, "application/json");
  };

  const exportSvg = () => {
    const svg = run((editor) => editor.exportSvg());
    if (svg !== undefined) deliver(SVG_FILE, svg, "image/svg+xml");
  };

  const exportPng = (scale: number, transparent: boolean) => {
    const image = run((editor) => editor.exportImage(scale, transparent));
    if (!image) return;
    const filename = scale === 1 ? PNG_FILE : PNG_FILE.replace(".png", `@${scale}x.png`);
    encodePng(image)
      .then((png) => deliver(filename, png, "image/png"))
      .catch(() => setNotice("Could not encode a PNG"));
  };

  /** Switching tools finishes a pen path or point editing in progress. */
  const changeTool = (next: Tool) => run((editor) => editor.setTool(next));

  // What Escape and Enter back out of or finish is the core's rule.
  const escape = () => run((editor) => editor.escape());
  const enter = () => run((editor) => editor.enter());

  const nudge = (dx: number, dy: number) => run((editor) => editor.nudge(dx, dy));
  const arrows: Shortcut[] = (
    [
      ["arrowleft", -1, 0],
      ["arrowright", 1, 0],
      ["arrowup", 0, -1],
      ["arrowdown", 0, 1],
    ] as const
  ).flatMap(([key, x, y]) => [
    { key, repeat: true, run: () => nudge(x * NUDGE, y * NUDGE) },
    { key, shift: true, repeat: true, run: () => nudge(x * NUDGE_LARGE, y * NUDGE_LARGE) },
  ]);

  // The view: per-viewer state that the core keeps, like the selection.
  const viewActions: ViewActions = {
    zoomIn: () => run((editor) => editor.zoomIn()),
    zoomOut: () => run((editor) => editor.zoomOut()),
    zoomTo100: () => run((editor) => editor.zoomTo(1)),
    zoomToFit: () => run((editor) => editor.zoomToFit()),
    snapping: core?.snapping ?? true,
    toggleSnapping: () => run((editor) => editor.setSnapping(!editor.snapping)),
  };

  useShortcuts(
    [
      { key: "v", run: () => changeTool("select") },
      { key: "r", run: () => changeTool("rect") },
      { key: "o", run: () => changeTool("ellipse") },
      // Illustrator's letters for the same tools; free in both sets.
      { key: "m", run: () => changeTool("rect") },
      { key: "l", run: () => changeTool("ellipse") },
      { key: "p", run: () => changeTool("pen") },
      { key: "t", run: () => changeTool("text") },
      { key: "escape", run: escape },
      { key: "enter", run: enter },
      { key: "delete", run: () => run((editor) => editor.deleteSelection()) },
      { key: "backspace", run: () => run((editor) => editor.deleteSelection()) },
      { key: "a", mod: true, run: () => run((editor) => editor.selectAll()) },
      { key: "d", mod: true, run: layerActions.duplicate },
      // Photoshop's binding for the same thing.
      { key: "j", mod: true, run: layerActions.duplicate },
      { key: "g", mod: true, run: layerActions.group },
      { key: "g", mod: true, shift: true, run: layerActions.ungroup },
      // By position, not character: Shift turns "]" into "}" on most layouts.
      { code: "BracketRight", mod: true, run: () => layerActions.arrange("forward") },
      { code: "BracketLeft", mod: true, run: () => layerActions.arrange("backward") },
      { code: "BracketRight", mod: true, shift: true, run: () => layerActions.arrange("front") },
      { code: "BracketLeft", mod: true, shift: true, run: () => layerActions.arrange("back") },
      { key: "h", mod: true, shift: true, run: layerActions.toggleVisible },
      { key: "l", mod: true, shift: true, run: layerActions.toggleLocked },
      { key: "z", mod: true, run: undo },
      { key: "z", mod: true, shift: true, run: redo },
      { key: "y", mod: true, run: redo },
      { key: "s", mod: true, run: () => void save(true) },
      // "=" is where "+" lives unshifted; the numpad's "+" needs no Shift.
      { key: "=", mod: true, run: viewActions.zoomIn },
      { key: "+", mod: true, run: viewActions.zoomIn },
      { key: "+", mod: true, shift: true, run: viewActions.zoomIn },
      { key: "-", mod: true, run: viewActions.zoomOut },
      { key: "0", mod: true, keymap: "default", run: viewActions.zoomTo100 },
      { key: "0", mod: true, keymap: "illustrator", run: viewActions.zoomToFit },
      { key: "1", mod: true, keymap: "illustrator", run: viewActions.zoomTo100 },
      { code: "Digit1", shift: true, run: viewActions.zoomToFit },
      // Illustrator's Smart Guides toggle.
      { key: "u", mod: true, run: viewActions.toggleSnapping },
      { key: "o", mod: true, run: open },
      { key: "e", mod: true, shift: true, run: exportSvg },
      ...arrows,
    ],
    keymap,
  );

  return (
    <TooltipProvider delayDuration={400}>
      <div className="flex h-full flex-col">
        <Header
          desktop={platform?.desktop ?? false}
          onOpen={open}
          onDownload={downloadProject}
          onExportSvg={exportSvg}
          onExportPng={exportPng}
          keymap={keymap}
          onKeymap={setKeymap}
        />
        <input
          ref={fileInput}
          type="file"
          accept=".json,application/json"
          hidden
          onChange={(event) => {
            const file = event.target.files?.[0];
            // Reset so choosing the same file again still fires.
            event.target.value = "";
            if (file) void openFile(file);
          }}
        />

        {error && (
          <div
            className="bg-destructive/10 text-destructive border-destructive/20 flex items-center gap-2 border-b py-1 pr-1 pl-3"
            role="alert"
          >
            <p className="min-w-0 flex-1 truncate">{error}</p>
            <Button size="icon-xs" onClick={clearError} aria-label="Dismiss">
              <X />
            </Button>
          </div>
        )}

        <main className="flex min-h-0 flex-1">
          <LayerPanel layers={layers} actions={layerActions} />
          {/* The backdrop colour shows only until the core's first frame. */}
          <section className="bg-canvas-backdrop relative min-w-0 flex-1">
            <Stage
              editor={editor}
              revision={revision}
              run={run}
              tool={tool}
              nextFill={nextFill}
            />
            <ToolDock
              tool={tool}
              onToolChange={changeTool}
              onUndo={undo}
              onRedo={redo}
              canUndo={core?.canUndo ?? false}
              canRedo={core?.canRedo ?? false}
              keymap={keymap}
            />
          </section>
          <PropertiesPanel properties={properties} {...propertyActions} {...arrangeActions} />
        </main>

        <StatusBar
          artboard={artboard}
          zoom={zoom}
          viewActions={viewActions}
          keymap={keymap}
          layerCount={layerCount}
          selectionCount={selectionCount}
          hint={hintFor(tool, mode)}
          notice={notice}
        />
      </div>
    </TooltipProvider>
  );
}
