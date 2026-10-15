import { Node, mergeAttributes } from '@tiptap/core';
import { ReactNodeViewRenderer } from '@tiptap/react';
import { CalloutView } from './CalloutView';

declare module '@tiptap/core' {
  interface Commands<ReturnType> {
    callout: { toggleCallout: () => ReturnType };
  }
}

/**
 * A highlighted block with an emoji icon and a colour. Same content model as a quote: a paragraph,
 * then blocks. `reactView` swaps in the interactive icon picker (needs a React editor).
 */
export const Callout = Node.create<{ reactView: boolean }>({
  name: 'callout',
  group: 'block',
  content: 'paragraph block*',
  defining: true,

  addOptions() {
    return { reactView: false };
  },

  addAttributes() {
    return {
      icon: {
        default: '💡',
        parseHTML: (el) => el.getAttribute('data-icon') ?? '💡',
        renderHTML: (attrs) => ({ 'data-icon': attrs.icon as string }),
      },
      tone: {
        default: null,
        parseHTML: (el) => el.getAttribute('data-tone'),
        renderHTML: (attrs) => (attrs.tone ? { 'data-tone': attrs.tone as string } : {}),
      },
    };
  },

  parseHTML() {
    return [{ tag: 'div[data-type="callout"]' }];
  },

  renderHTML({ HTMLAttributes }) {
    return ['div', mergeAttributes(HTMLAttributes, { 'data-type': 'callout' }), 0];
  },

  addNodeView() {
    return this.options.reactView ? ReactNodeViewRenderer(CalloutView) : null;
  },

  addCommands() {
    return {
      toggleCallout:
        () =>
        ({ commands, editor }) =>
          editor.isActive('callout') ? commands.lift('callout') : commands.wrapIn('callout'),
    };
  },
});
