import { actions } from '../state/app';
import { ArchiveList } from './ArchiveList';
import { Icon } from './Icon';
import { PageTree } from './PageTree';

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
        <button
          className="icon-btn"
          aria-label="New page"
          title={`New page (${mod}N)`}
          onClick={() => void actions.createPage(null)}
        >
          <Icon name="plus" size={15} />
        </button>
      </div>
      <nav className="sidebar-scroll">
        <PageTree />
      </nav>
      <ArchiveList />
      <div className="sidebar-footer">
        <button className="footer-btn" onClick={() => actions.setSettings(true)}>
          <Icon name="settings" size={15} /> Settings
        </button>
      </div>
    </aside>
  );
}
