import StarterKit from '@tiptap/starter-kit';
import { TaskItem, TaskList } from '@tiptap/extension-list';
import { BlockId } from './blockId';
import { BlockMove } from './blockMove';
import { Callout } from './callout';
import { SlashCommands } from './slashMenu';

export const extensions = [
  StarterKit.configure({ link: { openOnClick: false } }),
  TaskList,
  TaskItem.configure({ nested: true }),
  Callout,
  BlockId,
  BlockMove,
  SlashCommands,
];
