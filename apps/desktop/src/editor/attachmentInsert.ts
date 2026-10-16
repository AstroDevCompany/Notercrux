import type { Editor } from '@tiptap/core';
import { api } from '../api';
import { actions } from '../state/app';

export const IMAGE_TYPE = /^image\/(png|jpe?g|gif|webp|avif|bmp)$/i;

/** Opens the system file picker and resolves to the chosen files (none if cancelled). */
export function pickFiles(accept: string): Promise<File[]> {
  return new Promise((resolve) => {
    const input = Object.assign(document.createElement('input'), {
      type: 'file',
      accept,
      multiple: true,
    });
    input.style.display = 'none';
    input.addEventListener('change', () => {
      resolve([...(input.files ?? [])]);
      input.remove();
    });
    input.addEventListener('cancel', () => {
      resolve([]);
      input.remove();
    });
    document.body.append(input);
    input.click();
  });
}

/**
 * Stores each file as an attachment and inserts an image or file block for it. Failures are reported
 * per file; the others still go in.
 */
export async function addFiles(editor: Editor, files: File[], pos?: number) {
  const failed: string[] = [];
  for (const file of files) {
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const meta = await api.addAttachment({ name: file.name, mime: file.type, bytes });
      const image = IMAGE_TYPE.test(meta.mime);
      const node = {
        type: image ? 'image' : 'file',
        attrs: {
          attachment: meta.id,
          name: meta.name,
          mime: meta.mime,
          size: meta.size,
          ...(image ? { caption: '' } : {}),
        },
      };
      const chain = editor.chain().focus();
      (pos === undefined ? chain.insertContent(node) : chain.insertContentAt(pos, node)).run();
      if (pos !== undefined) pos = editor.state.selection.to;
    } catch (e) {
      failed.push(`${file.name || 'file'}: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  if (failed.length) actions.notify('Some files could not be added.', failed);
}
