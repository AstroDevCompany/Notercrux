import { Extension } from '@tiptap/core';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { newId } from '@notercrux/document-model';

const TYPES = [
  'callout',
  'paragraph',
  'heading',
  'listItem',
  'taskItem',
  'blockquote',
  'codeBlock',
  'horizontalRule',
  'image',
  'file',
];

/**
 * Gives every block node a stable `blockId` attribute and repairs missing or duplicated ids
 * (e.g. after paste or split), so blocks keep identity across saves.
 */
export const BlockId = Extension.create({
  name: 'blockId',

  addGlobalAttributes() {
    return [
      {
        types: TYPES,
        attributes: {
          blockId: {
            default: null,
            parseHTML: () => null, // ids are never taken from pasted content
            renderHTML: (attrs) => (attrs.blockId ? { 'data-block-id': attrs.blockId } : {}),
          },
        },
      },
    ];
  },

  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: new PluginKey('blockId'),
        appendTransaction(_transactions, _old, state) {
          const seen = new Set<string>();
          const tr = state.tr;
          let changed = false;
          state.doc.descendants((node, pos) => {
            if (!TYPES.includes(node.type.name)) return;
            const id = node.attrs.blockId as string | null;
            if (!id || seen.has(id)) {
              // An attribute step (not setNodeMarkup) so a selected block stays selected.
              tr.setNodeAttribute(pos, 'blockId', newId());
              changed = true;
            } else {
              seen.add(id);
            }
          });
          if (!changed) return null;
          tr.setMeta('addToHistory', false);
          return tr;
        },
      }),
    ];
  },
});
