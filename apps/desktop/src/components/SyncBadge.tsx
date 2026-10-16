import { actions } from '../state/app';
import { ago, PHASE_LABEL, useSync } from '../state/sync';
import { Icon } from './Icon';

/** Small status line in the sidebar; hidden while sync is off. */
export function SyncBadge() {
  const s = useSync((x) => x.status);
  if (s.phase === 'disabled') return null;
  const detail =
    s.phase === 'idle' ? `Last sync ${ago(s.lastSyncAt)}` : (s.lastError ?? PHASE_LABEL[s.phase]);
  return (
    <button
      className={`footer-btn sync-badge phase-${s.phase}`}
      onClick={() => actions.setSettings(true)}
      title={detail}
      aria-label={`Sync: ${PHASE_LABEL[s.phase]}. ${detail}`}
    >
      <Icon name="sync" size={15} className={s.phase === 'syncing' ? 'spin' : undefined} />
      {PHASE_LABEL[s.phase]}
      {s.pendingOps > 0 && s.phase !== 'syncing' && (
        <span className="badge-count">{s.pendingOps}</span>
      )}
    </button>
  );
}
