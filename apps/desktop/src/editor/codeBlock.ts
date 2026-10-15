import CodeBlockLowlight, {
  type CodeBlockLowlightOptions,
} from '@tiptap/extension-code-block-lowlight';
import { ReactNodeViewRenderer } from '@tiptap/react';
import { common, createLowlight } from 'lowlight';
import { CodeBlockView } from './CodeBlockView';

const lowlight = createLowlight(common);

/**
 * Code block with syntax highlighting (decorations only: the stored text is unchanged).
 * `reactView` swaps in the interactive language picker (needs a React editor).
 */
export const CodeBlock = CodeBlockLowlight.extend<
  CodeBlockLowlightOptions & { reactView: boolean }
>({
  addOptions() {
    return {
      ...this.parent?.(),
      lowlight: lowlight as CodeBlockLowlightOptions['lowlight'],
      defaultLanguage: null,
      reactView: false,
    } as CodeBlockLowlightOptions & { reactView: boolean };
  },
  addNodeView() {
    return this.options.reactView ? ReactNodeViewRenderer(CodeBlockView) : null;
  },
});
