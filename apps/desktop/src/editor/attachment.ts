import { Extension, Node, mergeAttributes } from '@tiptap/core';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { ReactNodeViewRenderer } from '@tiptap/react';
import { FileView, ImageView } from './AttachmentViews';
import { addFiles } from './attachmentInsert';

const attrs = (extra: Record<string, unknown> = {}) => ({
  attachment: { default: null },
  name: { default: '' },
  mime: { default: '' },
  size: { default: 0 },
  ...Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, { default: v }])),
});

const html = (type: string) => ({
  parseHTML: () => [{ tag: `figure[data-type="${type}"]` }],
  renderHTML: ({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) =>
    ['figure', mergeAttributes(HTMLAttributes, { 'data-type': type })] as [
      string,
      Record<string, unknown>,
    ],
});

/** An image stored as an attachment. The caption is the block's text. */
export const ImageBlock = Node.create<{ reactView: boolean }>({
  name: 'image',
  group: 'block',
  atom: true,
  draggable: true,
  selectable: true,
  addOptions: () => ({ reactView: false }),
  addAttributes: () => attrs({ caption: '' }),
  ...html('image'),
  addNodeView() {
    return this.options.reactView ? ReactNodeViewRenderer(ImageView) : null;
  },
});

/** Any other file stored as an attachment. */
export const FileBlock = Node.create<{ reactView: boolean }>({
  name: 'file',
  group: 'block',
  atom: true,
  draggable: true,
  selectable: true,
  addOptions: () => ({ reactView: false }),
  addAttributes: () => attrs(),
  ...html('file'),
  addNodeView() {
    return this.options.reactView ? ReactNodeViewRenderer(FileView) : null;
  },
});

/** Pasting or dropping files adds them as attachments. */
export const AttachmentDrop = Extension.create({
  name: 'attachmentDrop',
  addProseMirrorPlugins() {
    const editor = this.editor;
    return [
      new Plugin({
        key: new PluginKey('attachmentDrop'),
        props: {
          handlePaste(_view, event) {
            const files = [...(event.clipboardData?.files ?? [])];
            if (!files.length) return false;
            void addFiles(editor, files);
            return true;
          },
          handleDrop(view, event) {
            const files = [...(event.dataTransfer?.files ?? [])];
            if (!files.length) return false;
            event.preventDefault();
            const at = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos;
            void addFiles(editor, files, at);
            return true;
          },
        },
      }),
    ];
  },
});
