/**
 * Fonts for text on the artboard: which families the app offers, where
 * their files are, and fetching what the core asks for.
 *
 * The core sets text, and says which characters it could not
 * (`missingGlyphs`). This module fetches the slices of a family that cover
 * them — web fonts come sliced by unicode range, a hundred slices for
 * Chinese — decodes each from WOFF into the plain TrueType the core reads,
 * and hands it to the core, and to the browser for the text field used
 * while typing. Core does no IO; this is the IO.
 *
 * Only regular weight, for now: the core has no bold or italic yet.
 */
import interRanges from "@fontsource/inter/unicode.json";
import type { EditorHandle } from "@/editor";

// Vite turns each into a URL and copies the file into the build. Nothing is
// fetched until some text needs it.
const interFiles = import.meta.glob("/node_modules/@fontsource/inter/files/inter-*-400-normal.woff", {
  query: "?url",
  import: "default",
  eager: true,
}) as Record<string, string>;
const notoFiles = import.meta.glob(
  "/node_modules/@fontsource/noto-sans-tc/files/noto-sans-tc-*-400-normal.woff",
  { query: "?url", import: "default", eager: true },
) as Record<string, string>;

type Slice = {
  family: string;
  /** Its name in the package: "latin", or "[119]" for a numbered one. */
  key: string;
  url: string;
  /** As CSS writes it, for the browser's `FontFace`. */
  unicodeRange: string;
  ranges: [number, number][];
};

type Family = { name: string; slices: Slice[] };

/** The families offered, in the order they load: the core sets new text in the first. */
export const FAMILY_NAMES = ["Noto Sans TC", "Inter"] as const;
/** Where characters a family lacks come from: Noto Sans TC covers Latin and Chinese both. */
export const FALLBACK = "Noto Sans TC";

function family(name: string, prefix: string, unicode: Record<string, string>, files: Record<string, string>): Family {
  const slices: Slice[] = [];
  for (const [key, unicodeRange] of Object.entries(unicode)) {
    // "[12]" is a numbered slice, "latin" a named one.
    const file = `${prefix}-${key.replace(/[[\]]/g, "")}-400-normal.woff`;
    const url = Object.entries(files).find(([path]) => path.endsWith(`/${file}`))?.[1];
    if (!url) continue;
    const ranges = unicodeRange.split(",").map((part): [number, number] => {
      const [from, to = from] = part.trim().replace(/^U\+/i, "").split("-");
      return [parseInt(from, 16), parseInt(to, 16)];
    });
    slices.push({ family: name, key, url, unicodeRange, ranges });
  }
  return { name, slices };
}

/**
 * The families with their slices. Noto Sans TC's range table is 80 KB, so
 * it is a chunk of its own, fetched after start-up rather than with the app.
 */
let catalog: Promise<Family[]> | null = null;
function families(): Promise<Family[]> {
  catalog ??= import("@fontsource/noto-sans-tc/unicode.json").then((noto) => [
    family("Noto Sans TC", "noto-sans-tc", noto.default, notoFiles),
    family("Inter", "inter", interRanges, interFiles),
  ]);
  return catalog;
}

/**
 * Slices loaded or on their way, by URL, for each core: fonts belong to the
 * core they were given to, and a new one starts with none.
 */
const loadings = new WeakMap<EditorHandle, Map<string, Promise<boolean>>>();

function loadingFor(editor: EditorHandle): Map<string, Promise<boolean>> {
  let loading = loadings.get(editor);
  if (!loading) loadings.set(editor, (loading = new Map()));
  return loading;
}

function covering(family: Family | undefined, code: number): Slice | undefined {
  return family?.slices.find((slice) => slice.ranges.some(([from, to]) => from <= code && code <= to));
}

/**
 * Fetch a slice, decode it, and give it to the core and the browser, once.
 * Resolves to whether this call added it.
 */
function load(editor: EditorHandle, slice: Slice): Promise<boolean> {
  const loading = loadingFor(editor);
  let pending = loading.get(slice.url);
  if (!pending) {
    pending = fetch(slice.url)
      .then((response) => {
        if (!response.ok) throw new Error(`${slice.url}: ${response.status}`);
        return response.arrayBuffer();
      })
      .then(woffToSfnt)
      .then((sfnt) => {
        editor.addFont(sfnt);
        if (typeof FontFace !== "undefined" && "fonts" in document) {
          document.fonts.add(new FontFace(slice.family, sfnt, { unicodeRange: slice.unicodeRange }));
        }
        return true;
      })
      .catch(() => {
        // Let a later request try again, rather than remember a failure.
        loading.delete(slice.url);
        return false;
      });
    loading.set(slice.url, pending);
    return pending;
  }
  return pending.then(() => false);
}

/**
 * Each family's Latin slice, one after another so they arrive in order: the
 * first family is what new text is set in. Asked for by name — the most
 * common Chinese slice covers ASCII too, and is five times the size.
 */
export async function loadBaseFonts(editor: EditorHandle): Promise<void> {
  const all = await families();
  for (const name of FAMILY_NAMES) {
    const latin = all.find((f) => f.name === name)?.slices.find((slice) => slice.key === "latin");
    if (latin) await load(editor, latin);
  }
}

/**
 * Fetch what the core could not set: for each character, the slice of its
 * family that has it, or else the fallback family's. Resolves to whether
 * any font arrived, so the caller can have the text laid out again.
 */
export async function loadMissingFonts(editor: EditorHandle): Promise<boolean> {
  const missing = Object.entries(editor.missingGlyphs());
  if (missing.length === 0) return false;
  const all = await families();
  const loading = loadingFor(editor);
  const wanted = new Set<Slice>();
  for (const [name, chars] of missing) {
    const own = all.find((f) => f.name === name);
    const fallback = all.find((f) => f.name === FALLBACK);
    for (const char of chars) {
      const code = char.codePointAt(0)!;
      const slice = covering(own, code) ?? covering(fallback, code);
      if (slice && !loading.has(slice.url)) wanted.add(slice);
    }
  }
  const added = await Promise.all([...wanted].map((slice) => load(editor, slice)));
  return added.some(Boolean);
}

/**
 * WOFF 1.0 to the sfnt it wraps: each table is zlib-compressed on its own,
 * and the browser's `DecompressionStream` undoes that — no inflater in the
 * wasm. (WOFF2's Brotli it cannot, which is why the WOFF files are used.)
 */
export async function woffToSfnt(woff: ArrayBuffer): Promise<Uint8Array<ArrayBuffer>> {
  const view = new DataView(woff);
  if (view.getUint32(0) !== 0x774f4646) throw new Error("not a WOFF file");
  const tables = view.getUint16(12);
  const out = new Uint8Array(view.getUint32(16));
  const sfnt = new DataView(out.buffer);
  sfnt.setUint32(0, view.getUint32(4)); // flavor
  sfnt.setUint16(4, tables);
  let power = 1;
  let log = 0;
  while (power * 2 <= tables) {
    power *= 2;
    log++;
  }
  sfnt.setUint16(6, power * 16);
  sfnt.setUint16(8, log);
  sfnt.setUint16(10, tables * 16 - power * 16);

  let offset = 12 + tables * 16;
  for (let i = 0; i < tables; i++) {
    const entry = 44 + i * 20;
    const [at, compressed, length] = [view.getUint32(entry + 4), view.getUint32(entry + 8), view.getUint32(entry + 12)];
    const record = 12 + i * 16;
    sfnt.setUint32(record, view.getUint32(entry)); // tag
    sfnt.setUint32(record + 4, view.getUint32(entry + 16)); // checksum
    sfnt.setUint32(record + 8, offset);
    sfnt.setUint32(record + 12, length);
    const data = new Uint8Array(woff, at, compressed);
    out.set(compressed < length ? await inflate(data) : data, offset);
    offset += (length + 3) & ~3;
  }
  return out;
}

async function inflate(data: Uint8Array<ArrayBuffer>): Promise<Uint8Array<ArrayBuffer>> {
  const stream = new Blob([data]).stream().pipeThrough(new DecompressionStream("deflate"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}
