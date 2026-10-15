import StarterKit from '@tiptap/starter-kit';
import { TaskItem, TaskList } from '@tiptap/extension-list';
import { BlockId } from './blockId';
import { BlockMove } from './blockMove';
import { Callout } from './callout';
import { PageLink, PageLinkPicker } from './pageLink';
import { SlashCommands } from './slashMenu';

/** Extensions for a page's editor. `currentPageId` keeps the page itself out of the link picker. */
export const createExtensions = (currentPageId: () => string | null = () => null) => [
  StarterKit.configure({ link: { openOnClick: false } }),
  TaskList,
  TaskItem.configure({ nested: true }),
  Callout,
  PageLink,
  BlockId,
  BlockMove,
  SlashCommands,
  PageLinkPicker.configure({ currentPageId }),
];

export const extensions = createExtensions();
