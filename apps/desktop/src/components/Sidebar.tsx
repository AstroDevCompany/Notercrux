import { actions } from '../state/app';
import { ArchiveList } from './ArchiveList';
import { Icon } from './Icon';
import { PageTree } from './PageTree';
import { SyncBadge } from './SyncBadge';

const mod = /Mac/i.test(navigator.platform) ? '⌘' : 'Ctrl+';

export function Sidebar() {
  return (
    <aside className="sidebar glass-sidebar" aria-label="Sidebar">
      <div className="sidebar-top" data-tauri-drag-region />
      <button className="search-btn" onClick={() => actions.setPalette(true)}>
        <Icon name="search" size={15} />
        <span>Search or jump to…</span>
        <kbd>{mod}K</kbd>
      </button>
      <div className="sidebar-section-head">
        <span>Pages</span>
        <span className="head-actions">
          <button
            className="icon-btn"
            aria-label="New database"
            title="New database"
            onClick={() => void actions.createDatabase(null)}
          >
            <Icon name="table" size={15} />
          </button>
          <button
            className="icon-btn"
            aria-label="New page"
            title={`New page (${mod}N)`}
            onClick={() => void actions.createPage(null)}
          >
            <Icon name="plus" size={15} />
          </button>
        </span>
      </div>
      <nav className="sidebar-scroll">
        <PageTree />
      </nav>
      <ArchiveList />
      <div className="sidebar-footer">
        <SyncBadge />
        <button className="footer-btn" onClick={() => actions.setSettings(true)}>
          <Icon name="settings" size={15} /> Settings
        </button>
      </div>
    </aside>
  );
}
