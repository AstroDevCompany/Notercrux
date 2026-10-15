import { newId } from './ids';
import {
  DOCUMENT_SCHEMA_VERSION,
  type Block,
  type BlockKind,
  type Document,
  type Mark,
} from './types';

/**
 * Markdown and JSON import/export for pages.
 *
 * Both directions return `warnings` for anything that could not be represented, so callers can tell
 * the user instead of silently losing content (blueprint §12).
 */
export interface Converted<T> {
  value: T;
  warnings: string[];
}

// ---- export ------------------------------------------------------------------------------------

const ESCAPE = /([\\`*_[\]~])/g;

function renderInline(block: Block, warnings: Set<string>): string {
  const chars = Array.from(block.text);
  const marks = block.marks ?? [];
  const cuts = new Set<number>([0, chars.length]);
  for (const m of marks) {
    cuts.add(Math.min(m.from, chars.length));
    cuts.add(Math.min(m.to, chars.length));
  }
  const points = [...cuts].sort((a, b) => a - b);
  let out = '';
  for (let i = 0; i < points.length - 1; i++) {
    const [a, b] = [points[i]!, points[i + 1]!];
    if (a === b) continue;
    const active = marks.filter((m) => m.from <= a && m.to >= b);
    const raw = chars.slice(a, b).join('');
    const isCode = active.some((m) => m.type === 'code');
    let seg = isCode ? raw : raw.replace(ESCAPE, '\\$1');
    seg = seg.replace(/\n/g, '  \n');
    if (isCode) seg = '`' + seg + '`';
    if (active.some((m) => m.type === 'strike')) seg = `~~${seg}~~`;
    if (active.some((m) => m.type === 'italic')) seg = `*${seg}*`;
    if (active.some((m) => m.type === 'bold')) seg = `**${seg}**`;
    const link = active.find((m) => m.type === 'link');
    if (link) seg = `[${seg}](${String(link.attrs?.href ?? '')})`;
    if (active.some((m) => m.type === 'underline'))
      warnings.add('Underline is not supported in Markdown and was dropped.');
    out += seg;
  }
  return out;
}

function lineStart(text: string): string {
  // Stop plain text from being re-read as block syntax.
  return text.replace(/^(\s*)(#{1,6}\s|>|[-+]\s|\d+[.)]\s|---|\*\*\*|___)/, '$1\\$2');
}

function renderBlocks(blocks: Block[], depth: number, out: string[], warnings: Set<string>): void {
  const indent = '  '.repeat(depth);
  let number = 0;
  blocks.forEach((b, i) => {
    const prev = blocks[i - 1];
    number =
      b.type === 'numberedListItem' ? (prev?.type === 'numberedListItem' ? number + 1 : 1) : 0;
    const isList = (k?: BlockKind) =>
      k === 'bulletedListItem' || k === 'numberedListItem' || k === 'todo';
    // Consecutive items of the same list kind stay tight; everything else is separated.
    const sameList = isList(b.type) && b.type === prev?.type;
    if (i > 0 && !sameList && depth === 0) out.push('');
    const text = renderInline(b, warnings);
    switch (b.type) {
      case 'paragraph':
        out.push(indent + lineStart(text));
        break;
      case 'heading':
        out.push(`${'#'.repeat(Number(b.attrs?.level ?? 1))} ${text}`);
        break;
      case 'bulletedListItem':
        out.push(`${indent}- ${text}`);
        break;
      case 'numberedListItem':
        out.push(`${indent}${number}. ${text}`);
        break;
      case 'todo':
        out.push(`${indent}- [${b.attrs?.checked ? 'x' : ' '}] ${text}`);
        break;
      case 'quote':
      case 'callout': {
        if (b.type === 'callout') warnings.add('Callouts are exported as block quotes.');
        const icon = b.type === 'callout' && b.attrs?.icon ? `${String(b.attrs.icon)} ` : '';
        out.push(`> ${icon}${text.replace(/\n/g, '\n> ')}`);
        if (b.children?.length) {
          const inner: string[] = [];
          renderBlocks(b.children, 0, inner, warnings);
          out.push(...inner.map((l) => (l ? `> ${l}` : '>')));
        }
        return;
      }
      case 'code': {
        const fence = b.text.includes('```') ? '````' : '```';
        out.push(`${fence}${String(b.attrs?.language ?? '')}`, b.text, fence);
        break;
      }
      case 'divider':
        out.push('---');
        break;
      default:
        warnings.add(`Blocks of type "${b.type}" were exported as plain text.`);
        out.push(text);
    }
    if (b.children?.length && b.type !== 'code') renderBlocks(b.children, depth + 1, out, warnings);
  });
}

export function documentToMarkdown(title: string, doc: Document): Converted<string> {
  const warnings = new Set<string>();
  const lines: string[] = [];
  if (title.trim()) lines.push(`# ${title.trim()}`, '');
  renderBlocks(doc.blocks, 0, lines, warnings);
  return { value: lines.join('\n').replace(/\n+$/, '') + '\n', warnings: [...warnings] };
}

// ---- import ------------------------------------------------------------------------------------

interface Inline {
  text: string;
  marks: Mark[];
}

/** Parses the supported inline subset. Offsets are code points. */
function parseInline(src: string, warnings: Set<string>): Inline {
  const s = Array.from(src);
  const marks: Mark[] = [];
  let text = '';
  let len = 0;
  const push = (chunk: string) => {
    text += chunk;
    len += Array.from(chunk).length;
  };
  const find = (token: string, from: number): number => {
    const t = Array.from(token);
    for (let j = from; j <= s.length - t.length; j++) {
      if (s[j] === '\\') {
        j++;
        continue;
      }
      if (t.every((c, k) => s[j + k] === c)) return j;
    }
    return -1;
  };
  const wrap = (inner: string, type: string, attrs?: Record<string, unknown>) => {
    const start = len;
    const parsed = parseInline(inner, warnings);
    push(parsed.text);
    for (const m of parsed.marks) marks.push({ ...m, from: m.from + start, to: m.to + start });
    if (len > start) marks.push({ from: start, to: len, type, ...(attrs && { attrs }) });
  };

  let i = 0;
  while (i < s.length) {
    const c = s[i]!;
    const two = c + (s[i + 1] ?? '');
    if (c === '\\' && i + 1 < s.length) {
      push(s[i + 1]!);
      i += 2;
    } else if (c === '`') {
      let n = 1;
      while (s[i + n] === '`') n++;
      const j = find('`'.repeat(n), i + n);
      if (j < 0) {
        push(c);
        i++;
      } else {
        const start = len;
        push(
          s
            .slice(i + n, j)
            .join('')
            .replace(/^ (.*) $/, '$1'),
        );
        marks.push({ from: start, to: len, type: 'code' });
        i = j + n;
      }
    } else if ((c === '!' && s[i + 1] === '[') || c === '[') {
      const isImage = c === '!';
      const open = isImage ? i + 1 : i;
      const mid = find('](', open + 1);
      const end = mid < 0 ? -1 : find(')', mid + 2);
      if (mid < 0 || end < 0) {
        push(c);
        i++;
      } else {
        const label = s.slice(open + 1, mid).join('');
        const href =
          s
            .slice(mid + 2, end)
            .join('')
            .trim()
            .split(/\s+/)[0] ?? '';
        if (isImage) {
          warnings.add('Images are not supported; they were imported as links.');
          wrap(label || href, 'link', { href });
        } else {
          wrap(label, 'link', { href });
        }
        i = end + 1;
      }
    } else if (two === '**' || two === '__' || two === '~~') {
      const j = find(two, i + 2);
      if (j < 0) {
        push(two);
        i += 2;
      } else {
        wrap(s.slice(i + 2, j).join(''), two === '~~' ? 'strike' : 'bold');
        i = j + 2;
      }
    } else if ((c === '*' || c === '_') && !(c === '_' && /[\p{L}\p{N}]/u.test(s[i - 1] ?? ''))) {
      const j = find(c, i + 1);
      if (j < 0 || j === i + 1) {
        push(c);
        i++;
      } else {
        wrap(s.slice(i + 1, j).join(''), 'italic');
        i = j + 1;
      }
    } else {
      push(c);
      i++;
    }
  }
  return { text, marks: marks.sort((a, b) => a.from - b.from || a.to - b.to) };
}

function block(
  kind: BlockKind,
  raw: string,
  warnings: Set<string>,
  attrs?: Record<string, unknown>,
): Block {
  const { text, marks } = parseInline(raw, warnings);
  const b: Block = { id: newId(), type: kind, text };
  if (attrs) b.attrs = attrs;
  if (marks.length) b.marks = marks;
  return b;
}

const LIST_RE = /^(\s*)([-*+]|\d+[.)])\s+(?:\[([ xX])\]\s+)?(.*)$/;
const indentOf = (ws: string) => ws.replace(/\t/g, '    ').length;

export function markdownToDocument(
  markdown: string,
): Converted<{ title: string | null; document: Document }> {
  const warnings = new Set<string>();
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let listStack: { indent: number; block: Block }[] = [];
  let para: string[] = [];
  let i = 0;

  const flushPara = () => {
    if (!para.length) return;
    let raw = para[0]!;
    for (let k = 1; k < para.length; k++) {
      const hard = /( {2,}|\\)$/.test(raw);
      raw = raw.replace(/( {2,}|\\)$/, '') + (hard ? '\n' : ' ') + para[k]!.trimStart();
    }
    blocks.push(block('paragraph', raw.trimEnd(), warnings));
    para = [];
  };

  while (i < lines.length) {
    const line = lines[i]!;
    const fence = /^\s*(`{3,}|~{3,})\s*([^\s`]*)/.exec(line);
    if (fence) {
      flushPara();
      listStack = [];
      const marker = fence[1]!;
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i]!.trim().startsWith(marker)) body.push(lines[i++]!);
      i++; // closing fence (or end of input)
      const code: Block = { id: newId(), type: 'code', text: body.join('\n') };
      if (fence[2]) code.attrs = { language: fence[2] };
      blocks.push(code);
      continue;
    }
    if (!line.trim()) {
      flushPara();
      listStack = [];
      i++;
      continue;
    }
    const heading = /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(line);
    if (heading) {
      flushPara();
      listStack = [];
      const level = heading[1]!.length;
      if (level > 3) warnings.add('Headings deeper than level 3 were imported as level 3.');
      blocks.push(block('heading', heading[2]!, warnings, { level: Math.min(level, 3) }));
      i++;
      continue;
    }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      flushPara();
      listStack = [];
      blocks.push({ id: newId(), type: 'divider', text: '' });
      i++;
      continue;
    }
    if (/^\s*>/.test(line)) {
      flushPara();
      listStack = [];
      const quoted: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i]!))
        quoted.push(lines[i++]!.replace(/^\s*>\s?/, ''));
      blocks.push(block('quote', quoted.join('\n'), warnings));
      continue;
    }
    const li = LIST_RE.exec(line);
    if (li) {
      flushPara();
      const indent = indentOf(li[1]!);
      const kind: BlockKind =
        li[3] !== undefined ? 'todo' : /^\d/.test(li[2]!) ? 'numberedListItem' : 'bulletedListItem';
      const item = block(
        kind,
        li[4]!,
        warnings,
        kind === 'todo' ? { checked: li[3] !== ' ' } : undefined,
      );
      while (listStack.length && listStack[listStack.length - 1]!.indent >= indent) listStack.pop();
      const parent = listStack[listStack.length - 1]?.block;
      if (parent) (parent.children ??= []).push(item);
      else blocks.push(item);
      listStack.push({ indent, block: item });
      i++;
      continue;
    }
    if (/^\s*\|.*\|\s*$/.test(line)) {
      warnings.add('Tables are not supported; table rows were imported as plain text.');
    }
    if (/^\s*<[a-zA-Z!/]/.test(line))
      warnings.add('HTML is not supported and was imported as plain text.');
    listStack = [];
    para.push(line);
    i++;
  }
  flushPara();

  // A leading level-1 heading doubles as the page title.
  let title: string | null = null;
  const first = blocks[0];
  if (first?.type === 'heading' && first.attrs?.level === 1 && !first.children) {
    title = first.text;
    blocks.shift();
  }
  return {
    value: { title, document: { version: DOCUMENT_SCHEMA_VERSION, blocks } },
    warnings: [...warnings],
  };
}

// ---- JSON (lossless) ---------------------------------------------------------------------------

export const JSON_FORMAT = 'notercrux-page';

export interface PageExport {
  format: typeof JSON_FORMAT;
  version: 1;
  title: string;
  icon: string | null;
  document: Document;
}

export function pageToJson(title: string, icon: string | null, document: Document): string {
  const payload: PageExport = { format: JSON_FORMAT, version: 1, title, icon, document };
  return JSON.stringify(payload, null, 2) + '\n';
}

export function pageFromJson(json: string): PageExport {
  const data = JSON.parse(json) as Partial<PageExport>;
  if (data.format !== JSON_FORMAT) throw new Error('This file is not a Notercrux page export.');
  if (data.version !== 1) throw new Error(`Unsupported export version ${String(data.version)}.`);
  if (!data.document || data.document.version > DOCUMENT_SCHEMA_VERSION) {
    throw new Error('This export was made by a newer version of Notercrux.');
  }
  return {
    format: JSON_FORMAT,
    version: 1,
    title: data.title ?? '',
    icon: data.icon ?? null,
    document: data.document,
  };
}
