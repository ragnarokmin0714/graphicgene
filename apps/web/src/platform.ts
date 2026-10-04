/**
 * Where the app is running, and the IO that differs between places: a
 * browser tab keeps the project in IndexedDB and hands files over as
 * downloads; the desktop app keeps it in a file and saves through the
 * system's dialogs.
 *
 * Everything else — the core, the canvas, every panel — is the same code in
 * both. The core never sees this: it hands over JSON text and bytes, and
 * this decides where they go.
 */
import { download } from "@/files";
import { readProject, writeProject } from "@/storage";

/** A project file the user picked to open. */
export type OpenedFile = { name: string; text: string };

export type Platform = {
  /** Running in the desktop app rather than a browser tab. */
  readonly desktop: boolean;
  /** The autosaved project, or null if there is none yet. */
  readProject(): Promise<string | null>;
  writeProject(json: string): Promise<void>;
  /**
   * Hand the user a file, under `name` unless they choose another. Resolves
   * to the name it went out under, or null if they cancelled.
   */
  saveFile(name: string, data: string | Blob, type: string): Promise<string | null>;
  /**
   * The desktop's open dialog. A browser has none to call: the page's own
   * file input opens files there.
   */
  openProject?(): Promise<OpenedFile | null>;
};

const web: Platform = {
  desktop: false,
  readProject,
  writeProject,
  saveFile: async (name, data, type) => {
    download(name, data, type);
    return name;
  },
};

/**
 * The platform this page runs on. The desktop shell's code is a chunk of
 * its own, fetched only inside the desktop app.
 */
export async function detectPlatform(): Promise<Platform> {
  // What `isTauri()` in @tauri-apps/api checks, without loading the package.
  if (!(globalThis as { isTauri?: unknown }).isTauri) return web;
  const { desktop } = await import("@/desktop");
  return desktop;
}
