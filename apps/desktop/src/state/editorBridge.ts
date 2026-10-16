/** Lets non-editor code (export, page switches) wait for the open page's pending autosave. */
let flusher: (() => Promise<void>) | null = null;

export const setFlusher = (fn: (() => Promise<void>) | null) => {
  flusher = fn;
};
export const flushEditor = async () => {
  await flusher?.();
};

/** Called when another device changed documents (`page:<id>` keys) that are now stored locally. */
type RemoteListener = (documentsChanged: string[]) => void;
const remoteListeners = new Set<RemoteListener>();

export const onRemoteDocumentChange = (fn: RemoteListener) => {
  remoteListeners.add(fn);
  return () => {
    remoteListeners.delete(fn);
  };
};
export const announceRemoteChange = (documentsChanged: string[]) =>
  remoteListeners.forEach((l) => l(documentsChanged));
