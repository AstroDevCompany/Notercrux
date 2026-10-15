import { newId } from './ids';
import {
  DOCUMENT_SCHEMA_VERSION,
  type Block,
  type BlockKind,
  type Document,
  type Mark,
  type PMNode,
} from './types';

/**
 * Conversion between ProseMirror/Tiptap JSON (the editing representation) and the Notercrux
 * `Document` (the canonical, persisted, syncable representation).
 *
 * Unknown nodes or marks throw `UnsupportedContentError`: silently dropping content is never
 * acceptable (blueprint §5, §12).
 */
export class UnsupportedContentError extends Error {
  constructor(what: string) {
    super(`Unsupported content: ${what}`);
    this.name = 'UnsupportedContentError';
  }
}

const SUPPORTED_MARKS = new Set(['bold', 'italic', 'strike', 'code', 'underline', 'link']);

// ---- ProseMirror -> Document -------------------------------------------------------------------

function idOf(node: PMNode): string {
  const id = node.attrs?.blockId;
  return typeof id === 'string' && id ? id : newId();
}

function inline(nodes: PMNode[] | undefined): { text: string; marks: Mark[] } {
  let text = '';
  let len = 0; // code points
  const open = new Map<string, Mark>();
  const marks: Mark[] = [];
  const key = (m: { type: string; attrs?: Record<string, unknown> }) =>
    m.type + JSON.stringify(m.attrs ?? {});

  for (const n of nodes ?? []) {
    let chunk: string;
    let active: { type: string; attrs?: Record<string, unknown> }[] = [];
    if (n.type === 'text') {
      chunk = n.text ?? '';
      active = n.marks ?? [];
    } else if (n.type === 'hardBreak') {
      chunk = '\n';
    } else {
      throw new UnsupportedContentError(`inline node "${n.type}"`);
    }
    const activeKeys = new Set(active.map(key));
    for (const [k, m] of open) {
      if (!activeKeys.has(k)) {
        m.to = len;
        marks.push(m);
        open.delete(k);
      }
    }
    for (const m of active) {
      if (!SUPPORTED_MARKS.has(m.type)) throw new UnsupportedContentError(`mark "${m.type}"`);
      if (!open.has(key(m))) {
        const mark: Mark = { from: len, to: len, type: m.type };
        if (m.attrs && Object.keys(m.attrs).length) mark.attrs = m.attrs;
        open.set(key(m), mark);
      }
    }
    text += chunk;
    len += Array.from(chunk).length;
  }
  for (const m of open.values()) {
    m.to = len;
    marks.push(m);
  }
  return { text, marks: marks.filter((m) => m.to > m.from).sort((a, b) => a.from - b.from) };
}

function leaf(kind: BlockKind, node: PMNode, attrs?: Record<string, unknown>): Block {
  const { text, marks } = inline(node.content);
  const block: Block = { id: idOf(node), type: kind, text };
  if (attrs && Object.keys(attrs).length) block.attrs = attrs;
  if (marks.length) block.marks = marks;
  return block;
}

function listItem(kind: BlockKind, item: PMNode, attrs?: Record<string, unknown>): Block {
  const [head, ...rest] = item.content ?? [];
  const block: Block =
    head?.type === 'paragraph'
      ? { ...leaf(kind, head, attrs), id: idOf(item) }
      : { id: idOf(item), type: kind, text: '' };
  const tail = head?.type === 'paragraph' ? rest : (item.content ?? []);
  const children = tail.flatMap(toBlocks);
  if (children.length) block.children = children;
  return block;
}

function toBlocks(node: PMNode): Block[] {
  switch (node.type) {
    case 'paragraph':
      return [leaf('paragraph', node)];
    case 'heading':
      return [leaf('heading', node, { level: node.attrs?.level ?? 1 })];
    case 'bulletList':
      return (node.content ?? []).map((i) => listItem('bulletedListItem', i));
    case 'orderedList':
      return (node.content ?? []).map((i) => listItem('numberedListItem', i));
    case 'taskList':
      return (node.content ?? []).map((i) =>
        listItem('todo', i, { checked: Boolean(i.attrs?.checked) }),
      );
    case 'blockquote': {
      const [head, ...rest] = node.content ?? [];
      const q: Block =
        head?.type === 'paragraph'
          ? { ...leaf('quote', head), id: idOf(node) }
          : { id: idOf(node), type: 'quote', text: '' };
      const tail = head?.type === 'paragraph' ? rest : (node.content ?? []);
      const children = tail.flatMap(toBlocks);
      if (children.length) q.children = children;
      return [q];
    }
    case 'codeBlock': {
      const text = (node.content ?? []).map((n) => n.text ?? '').join('');
      const attrs = node.attrs?.language ? { language: node.attrs.language } : undefined;
      return [{ id: idOf(node), type: 'code', text, ...(attrs && { attrs }) }];
    }
    case 'horizontalRule':
      return [{ id: idOf(node), type: 'divider', text: '' }];
    default:
      throw new UnsupportedContentError(`node "${node.type}"`);
  }
}

export function fromProseMirror(doc: PMNode): Document {
  if (doc.type !== 'doc') throw new UnsupportedContentError(`root "${doc.type}"`);
  return {
    version: DOCUMENT_SCHEMA_VERSION,
    blocks: (doc.content ?? []).flatMap(toBlocks),
  };
}

// ---- Document -> ProseMirror -------------------------------------------------------------------

function pmInline(block: Block): PMNode[] {
  const chars = Array.from(block.text);
  const cuts = new Set<number>([0, chars.length]);
  for (const m of block.marks ?? []) {
    cuts.add(Math.min(m.from, chars.length));
    cuts.add(Math.min(m.to, chars.length));
  }
  const points = [...cuts].sort((a, b) => a - b);
  const out: PMNode[] = [];
  for (let i = 0; i < points.length - 1; i++) {
    const [a, b] = [points[i]!, points[i + 1]!];
    if (a === b) continue;
    const marks = (block.marks ?? [])
      .filter((m) => m.from <= a && m.to >= b)
      .map((m) => ({ type: m.type, ...(m.attrs && { attrs: m.attrs }) }));
    const parts = chars.slice(a, b).join('').split('\n');
    parts.forEach((part, idx) => {
      if (idx > 0) out.push({ type: 'hardBreak' });
      if (part) out.push({ type: 'text', text: part, ...(marks.length && { marks }) });
    });
  }
  return out;
}

function withContent(type: string, attrs: Record<string, unknown>, content: PMNode[]): PMNode {
  const n: PMNode = { type, attrs };
  if (content.length) n.content = content;
  return n;
}

function pmParagraph(block: Block): PMNode {
  return withContent('paragraph', { blockId: block.id }, pmInline(block));
}

function pmItem(type: 'listItem' | 'taskItem', block: Block, attrs: Record<string, unknown> = {}) {
  const head = withContent('paragraph', {}, pmInline(block));
  const rest = (block.children ?? []).flatMap((_, i, all) => pmGroup(all, i));
  return withContent(type, { blockId: block.id, ...attrs }, [head, ...rest]);
}

const LIST_TYPES: Partial<Record<BlockKind, string>> = {
  bulletedListItem: 'bulletList',
  numberedListItem: 'orderedList',
  todo: 'taskList',
};

/** Converts the block at `i` (and, for list items, the run of siblings of the same kind). */
function pmGroup(blocks: Block[], i: number): PMNode[] {
  const b = blocks[i]!;
  const listType = LIST_TYPES[b.type];
  if (listType) {
    if (i > 0 && blocks[i - 1]!.type === b.type) return []; // already emitted with its run
    const run: Block[] = [];
    for (let j = i; j < blocks.length && blocks[j]!.type === b.type; j++) run.push(blocks[j]!);
    const items = run.map((r) =>
      b.type === 'todo'
        ? pmItem('taskItem', r, { checked: Boolean(r.attrs?.checked) })
        : pmItem('listItem', r),
    );
    return [{ type: listType, content: items }];
  }
  switch (b.type) {
    case 'paragraph':
      return [pmParagraph(b)];
    case 'heading':
      return [withContent('heading', { blockId: b.id, level: b.attrs?.level ?? 1 }, pmInline(b))];
    case 'quote': {
      const head = withContent('paragraph', {}, pmInline(b));
      const rest = (b.children ?? []).flatMap((_, k, all) => pmGroup(all, k));
      return [withContent('blockquote', { blockId: b.id }, [head, ...rest])];
    }
    case 'code':
      return [
        withContent(
          'codeBlock',
          { blockId: b.id, language: b.attrs?.language ?? null },
          b.text ? [{ type: 'text', text: b.text }] : [],
        ),
      ];
    case 'divider':
      return [{ type: 'horizontalRule', attrs: { blockId: b.id } }];
    default:
      throw new UnsupportedContentError(`block type "${b.type}"`);
  }
}

export function toProseMirror(doc: Document): PMNode {
  const content = doc.blocks.flatMap((_, i, all) => pmGroup(all, i));
  return { type: 'doc', content: content.length ? content : [{ type: 'paragraph' }] };
}
