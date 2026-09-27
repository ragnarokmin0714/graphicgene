/**
 * The canvas backdrop — the colour around the artboard — as sRGB bytes for
 * the core, which paints it into the pixels.
 *
 * It comes from the `--canvas-backdrop` token, so the theme stays the one
 * source: the browser resolves the token (an oklch colour) by filling a
 * one-pixel canvas with it and reading the pixel back.
 */

/** Used where no canvas can be read, such as in the jsdom UI check. */
const FALLBACK = { light: [235, 235, 235], dark: [18, 18, 18] } as const;

export function resolveBackdrop(): readonly [number, number, number] {
  const dark = document.documentElement.classList.contains("dark");
  try {
    const token = getComputedStyle(document.documentElement)
      .getPropertyValue("--canvas-backdrop")
      .trim();
    const probe = document.createElement("canvas").getContext("2d", {
      willReadFrequently: true,
    });
    if (token && probe && typeof probe.getImageData === "function") {
      probe.canvas.width = probe.canvas.height = 1;
      probe.fillStyle = token;
      probe.fillRect(0, 0, 1, 1);
      const [r, g, b] = probe.getImageData(0, 0, 1, 1).data;
      return [r, g, b];
    }
  } catch {
    // Fall through: some environments have no 2D canvas.
  }
  return dark ? FALLBACK.dark : FALLBACK.light;
}
