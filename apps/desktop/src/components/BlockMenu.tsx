import { useEffect, useReducer, useRef, useState } from 'react';
import type { Editor } from '@tiptap/core';
import {
  blockAt,
  blockAtPos,
  canMove,
  deleteBlock,
  duplicateBlock,
  kindAt,
  kindLabel,
  moveBlockBy,
  selectBlock,
  setBlockAttrs,
  turnInto,
  TURN_INTO,
} from '../editor/blockOps';
import { CALLOUT_TONES, CODE_LANGUAGES } from '../editor/languages';
import { IconPicker } from './EmojiPicker';
import { Icon, type IconName } from './Icon';
import { Popover, type Anchor } from './database/Popover';

/** Re-renders when the editor document or selection changes, so the menu reflects live state. */
function useEditorTick(editor: Editor) {
  const [, bump] = useReducer((n: number) => n + 1, 0);
  useEffect(() => {
    editor.on('transaction', bump);
    return () => void editor.off('transaction', bump);
  }, [editor]);
}

const MOD = /Mac/i.test(navigator.platform) ? '⌘' : 'Ctrl+';

/**
 * Options for one block: type-specific properties (code language, callout icon and colour, to-do
 * state), "turn into", and actions. Opened from the drag handle, the right-click menu or Enter on a
 * selected block.
 */
export function BlockMenu({
  editor,
  pos,
  anchor,
  onClose,
}: {
  editor: Editor;
  pos: number;
  anchor: Anchor;
  onClose: () => void;
}) {
  useEditorTick(editor);
  const ref = blockAtPos(editor, pos);
  // The block can disappear (undo, remote edit); close instead of acting on the wrong node.
  useEffect(() => {
    if (!ref || ref.pos !== pos) onClose();
  }, [ref, pos, onClose]);
  if (!ref || ref.pos !== pos) return null;

  const { node } = ref;
  const kind = kindAt(editor, ref);
  const type = node.type.name;
  const done = (fn: () => void) => () => {
    onClose();
    fn();
  };

  return (
    <Popover anchor={anchor} label="Block options" width={272} onClose={onClose}>
      <div className="block-menu">
        <div className="bm-head">{kindLabel(kind)}</div>

        {type === 'codeBlock' && (
          <label className="bm-row">
            <span>Language</span>
            <select
              className="pop-input"
              aria-label="Code language"
              value={(node.attrs.language as string | null) ?? ''}
              onChange={(e) => setBlockAttrs(editor, pos, { language: e.target.value || null })}
            >
              {!CODE_LANGUAGES.some(
                (l) => l.id === ((node.attrs.language as string | null) ?? ''),
              ) && (
                <option value={node.attrs.language as string}>
                  {node.attrs.language as string}
                </option>
              )}
              {CODE_LANGUAGES.map((l) => (
                <option key={l.id} value={l.id}>
                  {l.label}
                </option>
              ))}
            </select>
          </label>
        )}

        {type === 'callout' && (
          <CalloutProps
            editor={editor}
            pos={pos}
            icon={node.attrs.icon as string}
            tone={(node.attrs.tone as string | null) ?? 'default'}
          />
        )}

        {type === 'taskItem' && (
          <label className="bm-row">
            <span>Done</span>
            <input
              type="checkbox"
              className="cell-check"
              aria-label="Done"
              checked={node.attrs.checked === true}
              onChange={(e) => setBlockAttrs(editor, pos, { checked: e.target.checked })}
            />
          </label>
        )}

        {kind !== 'divider' && (
          <>
            <div className="bm-label">Turn into</div>
            <div className="bm-grid" role="group" aria-label="Turn into">
              {TURN_INTO.map((t) => (
                <button
                  key={t.kind}
                  className={`bm-chip${t.kind === kind ? ' is-on' : ''}`}
                  aria-pressed={t.kind === kind}
                  onClick={done(() => {
                    if (turnInto(editor, pos, t.kind)) {
                      const next = blockAt(editor, editor.state.selection.from);
                      if (next) selectBlock(editor, next.pos);
                    }
                  })}
                >
                  {t.label}
                </button>
              ))}
            </div>
          </>
        )}

        <div className="menu-sep" role="separator" />
        <Action
          icon="copy"
          label="Duplicate"
          kbd={`${MOD}D`}
          onClick={done(() => void duplicateBlock(editor, pos))}
        />
        <Action
          icon="back"
          rotate={90}
          label="Move up"
          kbd="⌥↑"
          disabled={!canMove(editor, pos, -1)}
          onClick={done(() => void moveBlockBy(editor, pos, -1))}
        />
        <Action
          icon="back"
          rotate={-90}
          label="Move down"
          kbd="⌥↓"
          disabled={!canMove(editor, pos, 1)}
          onClick={done(() => void moveBlockBy(editor, pos, 1))}
        />
        <Action
          icon="copy"
          label="Copy"
          onClick={done(() => {
            selectBlock(editor, pos);
            document.execCommand('copy');
          })}
        />
        <Action
          icon="trash"
          label="Delete"
          danger
          kbd="⌫"
          onClick={done(() => void deleteBlock(editor, pos))}
        />
      </div>
    </Popover>
  );
}

function Action({
  icon,
  label,
  kbd,
  danger,
  disabled,
  rotate,
  onClick,
}: {
  icon: IconName;
  label: string;
  kbd?: string;
  danger?: boolean;
  disabled?: boolean;
  rotate?: number;
  onClick: () => void;
}) {
  return (
    <button
      className={`bm-action${danger ? ' is-danger' : ''}`}
      disabled={disabled}
      onClick={onClick}
    >
      <span
        style={rotate ? { display: 'inline-grid', transform: `rotate(${rotate}deg)` } : undefined}
      >
        <Icon name={icon} size={15} />
      </span>
      <span className="bm-action-label">{label}</span>
      {kbd && <kbd>{kbd}</kbd>}
    </button>
  );
}

function CalloutProps({
  editor,
  pos,
  icon,
  tone,
}: {
  editor: Editor;
  pos: number;
  icon: string;
  tone: string;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  return (
    <>
      <div className="bm-row">
        <span>Icon</span>
        <button
          ref={anchor}
          className="callout-icon bm-icon"
          aria-label="Change icon"
          aria-haspopup="dialog"
          onClick={() => setOpen(!open)}
        >
          {icon || '💡'}
        </button>
      </div>
      <div className="bm-row">
        <span>Color</span>
        <div className="tone-row" role="radiogroup" aria-label="Color">
          {CALLOUT_TONES.map((t) => (
            <button
              key={t.id}
              role="radio"
              aria-checked={tone === t.id}
              aria-label={t.label}
              title={t.label}
              className={`swatch${tone === t.id ? ' is-on' : ''}`}
              style={{ background: t.swatch }}
              onClick={() => setBlockAttrs(editor, pos, { tone: t.id === 'default' ? null : t.id })}
            />
          ))}
        </div>
      </div>
      {open && (
        <IconPicker
          anchor={anchor}
          icon={icon}
          onIcon={(i) => setBlockAttrs(editor, pos, { icon: i })}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}
