/** Lets non-editor code (export, page switches) wait for the open page's pending autosave. */
let flusher: (() => Promise<void>) | null = null;

export const setFlusher = (fn: (() => Promise<void>) | null) => {
  flusher = fn;
};
export const flushEditor = async () => {
  await flusher?.();
};
