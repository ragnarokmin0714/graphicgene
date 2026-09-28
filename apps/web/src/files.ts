import type { ExportedImage } from "@/editor";

/** Hand the user a file to save. The browser decides where it goes. */
export function download(filename: string, data: string | Blob, type: string): void {
  const url = URL.createObjectURL(new Blob([data], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  // The click starts the download synchronously; the URL can go afterwards.
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

/**
 * Encode exported pixels as PNG with the browser's own encoder, which the
 * wasm would otherwise have to carry. Straight alpha in, as `ImageData`
 * takes it; a semi-transparent pixel can come out a step off in colour,
 * since a canvas keeps its pixels premultiplied.
 */
export function encodePng({ width, height, pixels }: ExportedImage): Promise<Blob> {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (!context) return Promise.reject(new Error("no 2D canvas to encode with"));
  context.putImageData(new ImageData(pixels, width, height), 0, 0);
  return new Promise((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("PNG encoding failed"))), "image/png"),
  );
}
