import { useEffect, useRef, useState } from 'react';
import { EditorContent, useEditor } from '@tiptap/react';
import {
  fromProseMirror,
  toProseMirror,
  UnsupportedContentError,
  type Document,
  type Page,
} from '@notercrux/document-model';
import { api } from '../api';
import { extensions } from '../editor/extensions';
import { actions, pageLabel } from '../state/app';

type SaveState = 'saved' | 'saving' | 'error';

export function PageView({ page }: { page: Page }) {
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

  const editor = useEditor({
    extensions,
    content: toProseMirror(initial),
    editorProps: {
      attributes: { class: 'prose', 'aria-label': 'Page content', spellcheck: 'true' },
    },
    onUpdate: ({ editor }) => {
      dirty.current = true;
      setSave('saving');
      pending.current = async () => {
        try {
          // Convert first: unsupported content must never be silently dropped on save.
          const document = fromProseMirror(editor.getJSON());
          await api.saveDocument(page.id, document);
          dirty.current = false;
          setSave('saved');
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

  // Flush unsaved edits when leaving the page or closing the window.
  useEffect(() => {
    const flush = () => {
      clearTimeout(timer.current);
      if (dirty.current) void pending.current?.();
    };
    window.addEventListener('beforeunload', flush);
    window.addEventListener('blur', flush);
    return () => {
      window.removeEventListener('beforeunload', flush);
      window.removeEventListener('blur', flush);
      flush();
    };
  }, []);

  return (
    <article className="page" aria-label={pageLabel(page)}>
      <div className="page-inner">
        <TitleField page={page} onEnter={() => editor?.commands.focus('start')} />
        <EditorContent editor={editor} />
      </div>
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
      <input
        className="title"
        value={value}
        placeholder="Untitled"
        aria-label="Page title"
        onChange={(e) => setValue(e.target.value)}
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
