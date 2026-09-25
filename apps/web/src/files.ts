/** Hand the user a file to save. The browser decides where it goes. */
export function download(filename: string, text: string, type: string): void {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  // The click starts the download synchronously; the URL can go afterwards.
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
