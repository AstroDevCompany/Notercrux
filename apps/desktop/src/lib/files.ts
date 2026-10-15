import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '../api';

export interface PickedFile {
  name: string;
  contents: string;
}

const safeName = (s: string) =>
  s
    .replace(/[\\/:*?"<>|]+/g, '-')
    .replace(/\s+/g, ' ')
    .trim() || 'Untitled';

/** Saves text via a native save dialog (desktop) or a download (browser preview). Returns false if cancelled. */
export async function saveTextFile(
  baseName: string,
  extension: string,
  contents: string,
): Promise<boolean> {
  const defaultName = `${safeName(baseName)}.${extension}`;
  if (isTauri) return invoke<boolean>('export_file', { defaultName, extension, contents });
  const url = URL.createObjectURL(new Blob([contents], { type: 'text/plain;charset=utf-8' }));
  const a = Object.assign(document.createElement('a'), { href: url, download: defaultName });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return true;
}

/** Picks a text file via a native dialog (desktop) or a file input (browser preview). */
export async function pickTextFile(): Promise<PickedFile | null> {
  if (isTauri) return invoke<PickedFile | null>('import_file');
  return new Promise((resolve) => {
    const input = Object.assign(document.createElement('input'), {
      type: 'file',
      accept: '.md,.markdown,.txt,.json',
    });
    input.onchange = async () => {
      const f = input.files?.[0];
      resolve(f ? { name: f.name, contents: await f.text() } : null);
    };
    input.oncancel = () => resolve(null);
    input.click();
  });
}
