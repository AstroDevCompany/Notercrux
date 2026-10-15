/** Mirrors `crates/domain` (`Document`, `Block`, `Mark`). Keep the two in sync. */

export const DOCUMENT_SCHEMA_VERSION = 1;

export type BlockKind =
  | 'paragraph'
  | 'heading'
  | 'bulletedListItem'
  | 'numberedListItem'
  | 'todo'
  | 'quote'
  | 'callout'
  | 'code'
  | 'divider'
  | 'pageLink';

export interface Mark {
  /** Inclusive start, in Unicode code points. */
  from: number;
  /** Exclusive end, in Unicode code points. */
  to: number;
  type: string;
  attrs?: Record<string, unknown>;
}

export interface Block {
  id: string;
  type: BlockKind;
  attrs?: Record<string, unknown>;
  text: string;
  marks?: Mark[];
  children?: Block[];
}

export interface Document {
  version: number;
  blocks: Block[];
}

export interface Page {
  id: string;
  parentId: string | null;
  title: string;
  icon: string | null;
  position: number;
  createdAt: number;
  updatedAt: number;
  archivedAt: number | null;
}

/** ProseMirror / Tiptap JSON node, as produced by `editor.getJSON()`. */
export interface PMNode {
  type: string;
  attrs?: Record<string, unknown>;
  content?: PMNode[];
  text?: string;
  marks?: { type: string; attrs?: Record<string, unknown> }[];
}
