import { useEffect } from 'react';
import { CommandPalette } from './components/CommandPalette';
import { ConfirmDialog } from './components/ConfirmDialog';
import { PageView } from './components/PageView';
import { SettingsDialog } from './components/SettingsDialog';
import { Sidebar } from './components/Sidebar';
import { Toolbar } from './components/Toolbar';
import { useMaterial } from './hooks/useMaterial';
import { actions, useApp } from './state/app';

export default function App() {
  useMaterial();
  const sidebarOpen = useApp((s) => s.sidebarOpen);
  const loaded = useApp((s) => s.loaded);
  const pages = useApp((s) => s.pages);
  const currentId = useApp((s) => s.currentId);
  const error = useApp((s) => s.error);
  const notice = useApp((s) => s.notice);
  const page = pages.find((p) => p.id === currentId && !p.archivedAt);

  useEffect(() => void actions.init(), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const k = e.key.toLowerCase();
      if (k === 'k') actions.setPalette(!(document.querySelector('.palette') !== null));
      else if (k === 'n') void actions.createPage(null);
      else if (k === ',') actions.setSettings(true);
      else if (k === '\\') actions.toggleSidebar();
      else return;
      e.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  return (
    <div className={`shell${sidebarOpen ? '' : ' sidebar-closed'}`}>
      {sidebarOpen && <Sidebar />}
      <main className="content">
        <Toolbar />
        <div className="content-scroll">
          {page ? (
            <PageView key={page.id} page={page} />
          ) : (
            loaded && (
              <div className="page-state">
                <h2>Nothing open</h2>
                <p>Pick a page from the sidebar, or press ⌘/Ctrl+N to create one.</p>
                <button className="btn btn-primary" onClick={() => void actions.createPage(null)}>
                  New page
                </button>
              </div>
            )
          )}
        </div>
      </main>
      {error && (
        <div className="toast glass-popover" role="alert">
          <span>{error}</span>
          <button className="btn" onClick={() => actions.dismissError()}>
            Dismiss
          </button>
        </div>
      )}
      {notice && (
        <div className="toast glass-popover" role="status">
          <div>
            <div>{notice.text}</div>
            {notice.details.length > 0 && (
              <ul className="toast-details">
                {notice.details.map((d) => (
                  <li key={d}>{d}</li>
                ))}
              </ul>
            )}
          </div>
          <button className="btn" onClick={() => actions.dismissNotice()}>
            OK
          </button>
        </div>
      )}
      <CommandPalette />
      <SettingsDialog />
      <ConfirmDialog />
    </div>
  );
}
