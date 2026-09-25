/**
 * Where the project lives between visits: IndexedDB, one record.
 *
 * This is the app layer's IO — the core only hands over JSON text. IndexedDB
 * rather than localStorage because it is asynchronous (a large document does
 * not stall the UI while it is written) and not capped at a few megabytes.
 */

const DB_NAME = "graphicgene";
const STORE = "projects";
const AUTOSAVE_KEY = "autosave";
/** Where the pre-IndexedDB build kept its one manual save. */
const LEGACY_KEY = "graphicgene:project";

let connection: Promise<IDBDatabase> | null = null;

function database(): Promise<IDBDatabase> {
  connection ??= new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  }).catch((error: unknown) => {
    // Let the next call try again instead of caching the failure.
    connection = null;
    throw error;
  });
  return connection;
}

/** The autosaved project, or the legacy localStorage save if there is none. */
export async function readProject(): Promise<string | null> {
  const db = await database();
  const stored = await new Promise<unknown>((resolve, reject) => {
    const request = db.transaction(STORE).objectStore(STORE).get(AUTOSAVE_KEY);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  if (typeof stored === "string") return stored;
  try {
    return localStorage.getItem(LEGACY_KEY);
  } catch {
    return null;
  }
}

export async function writeProject(json: string): Promise<void> {
  const db = await database();
  await new Promise<void>((resolve, reject) => {
    const transaction = db.transaction(STORE, "readwrite");
    transaction.objectStore(STORE).put(json, AUTOSAVE_KEY);
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error);
  });
}
