import StarterKit from '@tiptap/starter-kit';
import { TaskItem, TaskList } from '@tiptap/extension-list';
import { BlockId } from './blockId';
import { BlockMove } from './blockMove';
import { BlockRangeSelection } from './blockRange';
import { BlockSelect } from './blockSelect';
import { Callout } from './callout';
import { CodeBlock } from './codeBlock';
import { PageLink, PageLinkPicker } from './pageLink';
import { SlashCommands } from './slashMenu';

/**
 * Extensions for a page's editor.
 * - `currentPageId` keeps the page itself out of the link picker.
 * - `views` turns on the interactive React node views (code language picker, callout icon picker);
 *   leave it off for schema-only use such as tests and conversions.
 */
export const createExtensions = (
  currentPageId: () => string | null = () => null,
  { views = false }: { views?: boolean } = {},
) => [
  StarterKit.configure({ link: { openOnClick: false }, codeBlock: false }),
  BlockRangeSelection,
  CodeBlock.configure({ reactView: views }),
  TaskList,
  TaskItem.configure({ nested: true }),
  Callout.configure({ reactView: views }),
  PageLink,
  BlockId,
  BlockMove,
  BlockSelect,
  SlashCommands,
  PageLinkPicker.configure({ currentPageId }),
];

export const extensions = createExtensions();
