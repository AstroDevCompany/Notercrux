import { useEffect, useState } from 'react';
import type { Editor } from '@tiptap/core';
import { getBlockRange } from '../editor/blockRange';

interface Pos {
  left: number;
  top: number;
}

const BUTTONS = [
  {
    name: 'bold',
    label: 'Bold',
    glyph: 'B',
    style: { fontWeight: 700 },
    toggle: (e: Editor) => e.chain().focus().toggleBold().run(),
  },
  {
    name: 'italic',
    label: 'Italic',
    glyph: 'I',
    style: { fontStyle: 'italic' },
    toggle: (e: Editor) => e.chain().focus().toggleItalic().run(),
  },
  {
    name: 'strike',
    label: 'Strikethrough',
    glyph: 'S',
    style: { textDecoration: 'line-through' },
    toggle: (e: Editor) => e.chain().focus().toggleStrike().run(),
  },
  {
    name: 'code',
    label: 'Code',
    glyph: '</>',
    style: { fontFamily: 'var(--font-mono)', fontSize: 12 },
    toggle: (e: Editor) => e.chain().focus().toggleCode().run(),
  },
] as const;

/** Floating glass toolbar over a non-empty text selection. */
export function SelectionToolbar({ editor }: { editor: Editor | null }) {
  const [pos, setPos] = useState<Pos | null>(null);
  const [, bump] = useState(0);
  const [linking, setLinking] = useState<string | null>(null);

  useEffect(() => {
    if (!editor) return;
    const update = () => {
      const { from, to, empty } = editor.state.selection;
      if (
        empty ||
        getBlockRange(editor.state) !== null || // blocks are selected: the block menu applies, not text formatting
        !editor.isFocused ||
        editor.isActive('codeBlock') ||
        editor.state.selection.constructor.name === 'NodeSelection'
      ) {
        if (!linking) setPos(null);
        return;
      }
      const a = editor.view.coordsAtPos(from);
      const b = editor.view.coordsAtPos(to);
      const mid = (Math.min(a.left, b.left) + Math.max(a.right, b.right)) / 2;
      setPos({
        left: Math.max(150, Math.min(mid, window.innerWidth - 150)),
        top: Math.min(a.top, b.top),
      });
      bump((n) => n + 1);
    };
    editor.on('selectionUpdate', update);
    editor.on('transaction', update);
    editor.on('focus', update);
    editor.on('blur', () => !linking && setPos(null));
    return () => {
      editor.off('selectionUpdate', update);
      editor.off('transaction', update);
      editor.off('focus', update);
    };
  }, [editor, linking]);

  if (!editor || !pos) return null;

  const applyLink = (href: string) => {
    const chain = editor.chain().focus().extendMarkRange('link');
    if (href.trim()) chain.setLink({ href: href.trim() }).run();
    else chain.unsetLink().run();
    setLinking(null);
  };

  return (
    <div
      className="sel-toolbar glass-popover"
      style={{ left: pos.left, top: Math.max(8, pos.top - 46) }}
      role="toolbar"
      aria-label="Text formatting"
      // Keep the editor selection while clicking toolbar buttons.
      onMouseDown={(e) => !(e.target instanceof HTMLInputElement) && e.preventDefault()}
    >
      {linking === null ? (
        <>
          {BUTTONS.map((b) => (
            <button
              key={b.name}
              className={`sel-btn${editor.isActive(b.name) ? ' is-on' : ''}`}
              aria-label={b.label}
              aria-pressed={editor.isActive(b.name)}
              title={b.label}
              style={b.style}
              onClick={() => b.toggle(editor)}
            >
              {b.glyph}
            </button>
          ))}
          <button
            className={`sel-btn${editor.isActive('link') ? ' is-on' : ''}`}
            aria-label="Link"
            aria-pressed={editor.isActive('link')}
            title="Link"
            onClick={() =>
              setLinking((editor.getAttributes('link').href as string | undefined) ?? '')
            }
          >
            🔗
          </button>
        </>
      ) : (
        <input
          className="sel-link"
          autoFocus
          placeholder="Paste a link, Enter to apply"
          aria-label="Link address"
          defaultValue={linking}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') applyLink(e.currentTarget.value);
            if (e.key === 'Escape') {
              setLinking(null);
              editor.commands.focus();
            }
          }}
        />
      )}
    </div>
  );
}
