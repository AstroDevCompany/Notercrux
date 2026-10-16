import { useEffect, useMemo, useRef, useState } from 'react';
import { EditorContent, useEditor } from '@tiptap/react';
import {
  fromProseMirror,
  toProseMirror,
  UnsupportedContentError,
  type Document,
  type Page,
} from '@notercrux/document-model';
import { api } from '../api';
import { createExtensions } from '../editor/extensions';
import { actions, pageLabel } from '../state/app';
import { onRemoteDocumentChange, setFlusher } from '../state/editorBridge';
import { replaceContent, sameDocument } from '../editor/replaceContent';
import { Backlinks } from './Backlinks';
import { BlockHandle } from './BlockHandle';
import { BlockRangeOverlay, MarqueeBand, useMarquee } from './MarqueeSelect';
import {
  blocksIn,
  deleteBlockRange,
  duplicateBlockRange,
  getBlockRange,
  turnRangeInto,
} from '../editor/blockRange';
import { TURN_INTO } from '../editor/blockOps';
import { BlockMenu } from './BlockMenu';
import { ContextMenu, type MenuItem } from './ContextMenu';
import type { Anchor } from './database/Popover';
import { BLOCK_MENU_EVENT } from '../editor/blockSelect';
import { buildEditorMenu } from '../editor/contextMenu';
import { TextSelection } from '@tiptap/pm/state';
import { DatabaseView } from './database/DatabaseView';
import { RecordProperties } from './database/RecordProperties';
import { PageLinkMenu } from './PageLinkMenu';
import { SelectionToolbar } from './SelectionToolbar';
import { SlashMenu } from './SlashMenu';

type SaveState = 'saved' | 'saving' | 'error';

export function PageView({ page }: { page: Page }) {
  if (page.kind === 'database') return <DatabaseView key={page.id} page={page} />;
  return <PageContent page={page} />;
}

function PageContent({ page }: { page: Page }) {
  const [doc, setDoc] = useState<Document | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setDoc(null);
    setLoadError(null);
    api
      .loadDocument(page.id)
      .then((d) => !cancelled && setDoc(d))
      .catch((e) => !cancelled && setLoadError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [page.id]);

  if (loadError) {
    return (
      <div className="page-state" role="alert">
        <h2>This page can’t be opened</h2>
        <p>{loadError}</p>
      </div>
    );
  }
  if (!doc) return <div className="page-state" aria-busy="true" />;
  return <Surface key={page.id} page={page} initial={doc} />;
}

function Surface({ page, initial }: { page: Page; initial: Document }) {
  const [save, setSave] = useState<SaveState>('saved');
  const [saveError, setSaveError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const dirty = useRef(false);
  const pending = useRef<() => Promise<void>>(undefined);
  /** The copy of the page the editor last matched; changes from other devices are merged against it. */
  const base = useRef<Document>(initial);
  /** Bumped on every edit, to tell whether the user kept typing while a save was in flight. */
  const version = useRef(0);
  const inner = useRef<HTMLDivElement>(null);
  const article = useRef<HTMLElement>(null);
  const [blockMenu, setBlockMenu] = useState<{ pos: number; anchor: Anchor } | null>(null);
  const [ctx, setCtx] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);

  const editor = useEditor({
    extensions: useMemo(() => createExtensions(() => page.id, { views: true }), [page.id]),
    content: toProseMirror(initial),
    editorProps: {
      attributes: { class: 'prose', 'aria-label': 'Page content', spellcheck: 'true' },
    },
    onUpdate: ({ editor }) => {
      dirty.current = true;
      version.current += 1;
      setSave('saving');
      pending.current = async () => {
        try {
          // Convert first: unsupported content must never be silently dropped on save.
          const sent = fromProseMirror(editor.getJSON());
          const sentVersion = version.current;
          // Saved on top of the page as it is now, so edits other devices made are never overwritten.
          const res = await api.saveDocumentRebased(page.id, base.current, sent);
          const untouched = sentVersion === version.current;
          if (sameDocument(res.document, sent)) {
            base.current = sent;
          } else if (untouched) {
            replaceContent(editor, res.document);
            base.current = res.document;
            if (res.conflicts > 0) {
              actions.notify('Another device edited the same text at the same time.', [
                'Your version is kept. The other version is in a callout at the end of the page.',
              ]);
            }
          } else {
            base.current = sent; // still typing: the next save merges the rest
          }
          if (untouched) dirty.current = false;
          setSave(untouched ? 'saved' : 'saving');
          setSaveError(null);
        } catch (e) {
          setSave('error');
          setSaveError(e instanceof UnsupportedContentError ? e.message : String(e));
        }
      };
      clearTimeout(timer.current);
      timer.current = setTimeout(() => void pending.current?.(), 350);
    },
  });

  // Another device changed this page: show it. Unsaved typing is saved first and merged.
  useEffect(() => {
    if (!editor) return;
    return onRemoteDocumentChange((keys) => {
      if (!keys.includes(`page:${page.id}`)) return;
      void (async () => {
        if (dirty.current) {
          clearTimeout(timer.current);
          await pending.current?.();
          return;
        }
        const seen = version.current;
        const fresh = await api.loadDocument(page.id);
        if (seen !== version.current || dirty.current) return; // typing started; its save will merge
        if (!sameDocument(fresh, base.current)) {
          replaceContent(editor, fresh);
          base.current = fresh;
        }
      })();
    });
  }, [editor, page.id]);

  // Enter on a selected block opens its options, anchored to the block.
  useEffect(() => {
    const dom = editor?.view.dom;
    if (!editor || !dom) return;
    const onEvent = (e: Event) => {
      const pos = (e as CustomEvent<{ pos: number }>).detail.pos;
      const el = editor.view.nodeDOM(pos);
      if (el instanceof HTMLElement) setBlockMenu({ pos, anchor: el });
    };
    dom.addEventListener(BLOCK_MENU_EVENT, onEvent);
    return () => dom.removeEventListener(BLOCK_MENU_EVENT, onEvent);
  }, [editor]);

  const marquee = useMarquee(editor, article);

  /** Right-click on a block selection: operate on the whole range, whatever was under the pointer. */
  const rangeMenu = (e: React.MouseEvent): boolean => {
    if (!editor) return false;
    const range = getBlockRange(editor.state);
    if (!range) return false;
    const blocks = blocksIn(editor.state, range);
    const first = editor.view.nodeDOM(blocks[0]!.pos);
    const last = editor.view.nodeDOM(blocks[blocks.length - 1]!.pos);
    if (!(first instanceof HTMLElement) || !(last instanceof HTMLElement)) return false;
    if (
      e.clientY < first.getBoundingClientRect().top ||
      e.clientY > last.getBoundingClientRect().bottom
    )
      return false;
    e.preventDefault();
    e.stopPropagation();
    const mod = /Mac/i.test(navigator.platform) ? '⌘' : 'Ctrl+';
    const run = (c: 'copy' | 'cut') => {
      editor.view.focus();
      document.execCommand(c);
    };
    setCtx({
      x: e.clientX,
      y: e.clientY,
      items: [
        {
          type: 'label',
          label: `${blocks.length} block${blocks.length === 1 ? '' : 's'} selected`,
        },
        { label: 'Copy', shortcut: `${mod}C`, onSelect: () => run('copy') },
        { label: 'Cut', shortcut: `${mod}X`, onSelect: () => run('cut') },
        {
          label: 'Duplicate',
          shortcut: `${mod}D`,
          onSelect: () => void duplicateBlockRange(editor),
        },
        {
          label: 'Turn into',
          children: TURN_INTO.map((t) => ({
            label: t.label,
            onSelect: () => void turnRangeInto(editor, t.kind),
          })),
        },
        { type: 'separator' },
        {
          label: 'Delete',
          danger: true,
          shortcut: '⌫',
          onSelect: () => void deleteBlockRange(editor),
        },
      ],
    });
    return true;
  };

  /** Right-click inside the page content: place the cursor if needed, then show the editor menu. */
  const onContextMenu = (e: React.MouseEvent) => {
    if (rangeMenu(e)) return;
    if (!editor || !(e.target instanceof HTMLElement) || !e.target.closest('.prose')) return;
    e.preventDefault();
    e.stopPropagation();
    const hit = editor.view.posAtCoords({ left: e.clientX, top: e.clientY });
    if (!hit) return;
    const { from, to, empty } = editor.state.selection;
    const insideSelection = !empty && hit.pos >= from && hit.pos <= to;
    if (!insideSelection) {
      editor.view.dispatch(
        editor.state.tr.setSelection(TextSelection.near(editor.state.doc.resolve(hit.pos))),
      );
    }
    editor.view.focus();
    const link = e.target.closest('a[data-page-id]');
    setCtx({
      x: e.clientX,
      y: e.clientY,
      items: buildEditorMenu(editor, {
        pos: hit.pos,
        linkPageId: link?.getAttribute('data-page-id') ?? null,
        openBlockMenu: (pos) =>
          setBlockMenu({
            pos,
            anchor: { getBoundingClientRect: () => new DOMRect(e.clientX, e.clientY, 0, 0) },
          }),
      }),
    });
  };

  // Flush unsaved edits when leaving the page, closing the window, or when another feature
  // (export) needs the latest content on disk.
  useEffect(() => {
    const flush = async () => {
      clearTimeout(timer.current);
      if (dirty.current) await pending.current?.();
    };
    setFlusher(flush);
    const onLeave = () => void flush();
    window.addEventListener('beforeunload', onLeave);
    window.addEventListener('blur', onLeave);
    return () => {
      window.removeEventListener('beforeunload', onLeave);
      window.removeEventListener('blur', onLeave);
      void flush();
      setFlusher(null);
    };
  }, []);

  return (
    <article
      ref={article}
      className="page"
      aria-label={pageLabel(page)}
      onContextMenu={onContextMenu}
    >
      <div className="page-inner" ref={inner}>
        <BlockRangeOverlay editor={editor} inner={inner} />
        <BlockHandle
          editor={editor}
          container={inner}
          frozen={blockMenu !== null}
          onOpenMenu={(pos, anchor) => setBlockMenu({ pos, anchor })}
        />
        <TitleField page={page} onEnter={() => editor?.commands.focus('start')} />
        <RecordProperties page={page} />
        <EditorContent editor={editor} />
        <Backlinks pageId={page.id} />
      </div>
      <MarqueeBand box={marquee} />
      <SelectionToolbar editor={editor} />
      {editor && blockMenu && (
        <BlockMenu
          editor={editor}
          pos={blockMenu.pos}
          anchor={blockMenu.anchor}
          onClose={() => {
            setBlockMenu(null);
            editor.view.focus();
          }}
        />
      )}
      {ctx && <ContextMenu x={ctx.x} y={ctx.y} items={ctx.items} onClose={() => setCtx(null)} />}
      <SlashMenu />
      <PageLinkMenu />
      <div className="save-state" role="status" aria-live="polite">
        {save === 'saving' && 'Saving…'}
        {save === 'saved' && 'Saved'}
        {save === 'error' && <span className="save-error">Not saved: {saveError}</span>}
      </div>
    </article>
  );
}

function TitleField({ page, onEnter }: { page: Page; onEnter: () => void }) {
  const [value, setValue] = useState(page.title);
  const latest = useRef(value);
  latest.current = value;
  const saved = useRef(page.title);
  const field = useRef<HTMLTextAreaElement>(null);

  // Grow with content so long titles wrap instead of being clipped.
  useEffect(() => {
    const el = field.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);

  // External renames (sidebar) update the field unless it holds unsaved typing.
  useEffect(() => {
    if (latest.current === saved.current) setValue(page.title);
    saved.current = page.title;
  }, [page.title]);

  const commit = () => {
    if (latest.current !== saved.current) {
      saved.current = latest.current;
      void actions.rename(page.id, latest.current);
    }
  };
  useEffect(() => commit, []);

  return (
    <div className="title-row">
      {page.icon && (
        <span className="title-icon" aria-hidden="true">
          {page.icon}
        </span>
      )}
      <textarea
        ref={field}
        className="title"
        rows={1}
        value={value}
        placeholder="Untitled"
        aria-label="Page title"
        onChange={(e) => setValue(e.target.value.replace(/\n/g, ' '))}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            commit();
            onEnter();
          }
        }}
      />
    </div>
  );
}
