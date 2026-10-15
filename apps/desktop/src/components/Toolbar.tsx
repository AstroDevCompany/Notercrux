import { actions, breadcrumb, canGo, pageLabel, useApp } from '../state/app';
import { formatBinding, useBindings } from '../state/shortcuts';
import { Icon } from './Icon';

/** Top bar of the content area. Doubles as the window drag region on the native app. */
export function Toolbar() {
  const pages = useApp((s) => s.pages);
  const currentId = useApp((s) => s.currentId);
  const sidebarOpen = useApp((s) => s.sidebarOpen);
  const history = useApp((s) => s.history);
  const bindings = useBindings();
  const trail = currentId ? breadcrumb(pages, currentId) : [];

  return (
    <header className="toolbar" data-tauri-drag-region>
      {!sidebarOpen && <div className="traffic-spacer" data-tauri-drag-region />}
      <button
        className="icon-btn"
        aria-label={sidebarOpen ? 'Hide sidebar' : 'Show sidebar'}
        aria-pressed={sidebarOpen}
        title="Toggle sidebar"
        onClick={() => actions.toggleSidebar()}
      >
        <Icon name="sidebar" size={17} />
      </button>
      <button
        className="icon-btn"
        aria-label="Go back"
        title={`Back (${formatBinding(bindings.back)})`}
        disabled={!canGo({ history }, -1)}
        onClick={() => actions.go(-1)}
      >
        <Icon name="back" size={17} />
      </button>
      <button
        className="icon-btn"
        aria-label="Go forward"
        title={`Forward (${formatBinding(bindings.forward)})`}
        disabled={!canGo({ history }, 1)}
        onClick={() => actions.go(1)}
      >
        <Icon name="forward" size={17} />
      </button>
      <nav aria-label="Breadcrumb" className="crumbs" data-tauri-drag-region>
        <ol>
          {trail.map((p, i) => (
            <li key={p.id}>
              {i > 0 && (
                <span aria-hidden="true" className="crumb-sep">
                  /
                </span>
              )}
              {i === trail.length - 1 ? (
                <span aria-current="page" className="crumb-current">
                  {p.icon} {pageLabel(p)}
                </span>
              ) : (
                <button className="crumb" onClick={() => actions.open(p.id)}>
                  {p.icon} {pageLabel(p)}
                </button>
              )}
            </li>
          ))}
        </ol>
      </nav>
      <button
        className="icon-btn"
        aria-label="Search"
        title="Search (⌘K)"
        onClick={() => actions.setPalette(true)}
      >
        <Icon name="search" size={17} />
      </button>
    </header>
  );
}
