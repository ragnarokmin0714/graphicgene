/**
 * The desktop app's IO: the commands in `apps/desktop/src-tauri`, which
 * keep the autosave in a file and read and write through the system's own
 * dialogs. Loaded only when the page runs inside the desktop app.
 */
import { invoke } from "@tauri-apps/api/core";
import type { OpenedFile, Platform } from "@/platform";

export const desktop: Platform = {
  desktop: true,
  readProject: () => invoke<string | null>("read_autosave"),
  writeProject: (json) => invoke<void>("write_autosave", { json }),
  openProject: () => invoke<OpenedFile | null>("open_project"),
  async saveFile(name, data) {
    // The bytes go as the raw body, not as a JSON array of numbers — a 3×
    // PNG would be megabytes of digits. The suggested name rides in a header.
    const bytes = typeof data === "string" ? new TextEncoder().encode(data) : new Uint8Array(await data.arrayBuffer());
    return invoke<string | null>("save_file", bytes, { headers: { "x-file-name": encodeURIComponent(name) } });
  },
};
