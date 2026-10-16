import { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import type { DeviceView, WorkspaceInfo } from '../api/types';
import { actions } from '../state/app';
import { ago, formatBytes, openSyncDialog, PHASE_LABEL, refreshSync, useSync } from '../state/sync';
import { Icon } from './Icon';

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function SyncSettings() {
  const status = useSync((s) => s.status);
  const secure = useSync((s) => s.secureStorage);
  const events = useSync((s) => s.events);
  const [devices, setDevices] = useState<DeviceView[]>([]);
  const [info, setInfo] = useState<WorkspaceInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showLog, setShowLog] = useState(false);
  const linked = status.phase !== 'disabled' && status.phase !== 'keysMissing';
  const online = linked && status.phase !== 'revoked';

  const load = useCallback(async () => {
    try {
      const [d, i] = await Promise.all([api.syncDevices(), api.syncInfo()]);
      setDevices(d);
      setInfo(i);
      setError(null);
    } catch (e) {
      setError(message(e));
    }
  }, []);

  useEffect(() => {
    void refreshSync();
  }, []);
  useEffect(() => {
    if (online) void load();
  }, [online, status.epoch, status.workspaceId, load]);

  const revoke = (d: DeviceView) =>
    actions.ask({
      title: `Remove “${d.label}”?`,
      message: d.current
        ? 'This device will stop syncing and keep its pages. Your other devices get a new encryption key.'
        : 'It will stop syncing and cannot read anything written from now on. Your other devices get a new encryption key.',
      confirmLabel: 'Remove',
      destructive: true,
      onConfirm: () => {
        void api
          .syncRevokeDevice(d.id)
          .then(() => (d.current ? api.syncLeaveLocal() : undefined))
          .then(() => refreshSync())
          .then(() => load())
          .catch((e) => setError(message(e)));
      },
    });

  const disconnect = () =>
    actions.ask({
      title: 'Disconnect this device?',
      message:
        'It stops syncing and keeps all its pages. If other devices remain, this one is removed from the workspace and the key is replaced.',
      confirmLabel: 'Disconnect',
      destructive: true,
      onConfirm: () => {
        void api
          .syncLeave()
          .then(() => refreshSync())
          .catch((e) => setError(message(e)));
      },
    });

  const deleteRemote = () =>
    actions.ask({
      title: 'Delete the workspace from the server?',
      message:
        'All encrypted data on the server is erased and every device stops syncing. Pages stay on each device. This cannot be undone.',
      confirmLabel: 'Delete from server',
      destructive: true,
      onConfirm: () => {
        void api
          .syncDeleteRemote()
          .then(() => refreshSync())
          .catch((e) => setError(message(e)));
      },
    });

  return (
    <section aria-label="Sync">
      <h3 className="dialog-sub">Sync</h3>

      {status.phase === 'disabled' && (
        <div className="setting">
          <div>
            <div className="setting-label">Sync across devices</div>
            <div className="setting-desc">
              End-to-end encrypted. The server only stores data it cannot read.
            </div>
          </div>
          <button className="btn btn-primary" onClick={() => openSyncDialog('wizard')}>
            Set up…
          </button>
        </div>
      )}

      {status.phase === 'keysMissing' && (
        <div className="sync-banner is-error" role="alert">
          <strong>Encryption keys missing.</strong>
          <span>{status.lastError}</span>
          <button className="btn" onClick={() => void api.syncLeaveLocal().then(refreshSync)}>
            Disconnect and recover
          </button>
        </div>
      )}

      {status.phase === 'revoked' && (
        <div className="sync-banner is-error" role="alert">
          <strong>This device was removed from the workspace.</strong>
          <span>
            Its pages are untouched, including changes that were never uploaded. Disconnect to keep
            them as a separate copy.
          </span>
          <button className="btn" onClick={() => void api.syncLeaveLocal().then(refreshSync)}>
            Keep my pages and disconnect
          </button>
        </div>
      )}

      {online && (
        <>
          <div className="setting sync-status">
            <div>
              <div className="setting-label">
                <span className={`dot dot-${status.phase}`} aria-hidden="true" />
                {PHASE_LABEL[status.phase]}
              </div>
              <div className="setting-desc sync-meta">
                <span>{status.server}</span>
                <span>Last sync: {ago(status.lastSyncAt)}</span>
                {status.pendingOps > 0 && (
                  <span>{status.pendingOps} changes waiting to upload</span>
                )}
                {status.attachmentsPending > 0 && (
                  <span>{status.attachmentsPending} files in transfer</span>
                )}
              </div>
            </div>
            <button className="btn" onClick={() => void api.syncNow()}>
              <Icon name="sync" size={14} /> Sync now
            </button>
          </div>
          {status.lastError && status.phase !== 'idle' && (
            <p className="form-error" role="alert">
              {status.lastError}
              {status.retryInSecs ? ` Trying again in ${status.retryInSecs} s.` : ''}
            </p>
          )}
          {status.attachmentsFailed > 0 && (
            <div className="sync-banner is-warn" role="alert">
              <span>{status.attachmentsFailed} file(s) could not be synced.</span>
              <button className="btn" onClick={() => void api.syncRetryAttachments()}>
                Try again
              </button>
            </div>
          )}
          {status.rejectedOps > 0 && (
            <p className="setting-desc" style={{ maxWidth: 'none' }}>
              {status.rejectedOps} change(s) from the server failed verification and were skipped.
            </p>
          )}
          {status.heldOps > 0 && (
            <p className="setting-desc" style={{ maxWidth: 'none' }}>
              {status.heldOps} change(s) come from a newer version of Notercrux. They are kept and
              will appear after you update the app.
            </p>
          )}

          <div className="sync-head">
            <h4>Devices</h4>
            <button className="btn" onClick={() => openSyncDialog('addDevice')}>
              Add device
            </button>
          </div>
          <ul className="device-list">
            {devices
              .filter((d) => !d.revoked)
              .map((d) => (
                <li key={d.id}>
                  <div>
                    <strong>{d.label}</strong>
                    {d.current && <span className="tag">This device</span>}
                    <div className="setting-desc">
                      {d.lastSeen ? `Active ${ago(d.lastSeen)}` : 'Not seen yet'}
                    </div>
                  </div>
                  <button className="btn" onClick={() => revoke(d)}>
                    Remove
                  </button>
                </li>
              ))}
          </ul>

          {info && (
            <p className="setting-desc" style={{ maxWidth: 'none' }}>
              Server storage: {formatBytes(info.usage.opBytes)} of{' '}
              {formatBytes(info.limits.maxOpBytesTotal)} for pages,{' '}
              {formatBytes(info.usage.blobBytes)} of {formatBytes(info.limits.maxBlobBytesTotal)}{' '}
              for files. Encryption key generation {info.epoch}.
            </p>
          )}
          {secure === 'file' && (
            <p className="form-error" role="status">
              The system keychain is not available, so this device&apos;s keys are kept in a private
              file instead. That is less safe if other programs run as you.
            </p>
          )}
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}

          <button className="link-btn" onClick={() => setShowLog(!showLog)} aria-expanded={showLog}>
            {showLog ? 'Hide activity' : 'Show activity'}
          </button>
          {showLog && (
            <ul className="sync-log" aria-label="Sync activity">
              {events.length === 0 && <li>Nothing yet.</li>}
              {[...events].reverse().map((e, i) => (
                <li key={i} className={`lvl-${e.level}`}>
                  <time>{new Date(e.at * 1000).toLocaleTimeString()}</time> {e.message}
                </li>
              ))}
            </ul>
          )}

          <div className="sync-danger">
            <button className="btn" onClick={disconnect}>
              Disconnect this device
            </button>
            <button className="btn btn-danger" onClick={deleteRemote}>
              Delete workspace from server
            </button>
          </div>
        </>
      )}
    </section>
  );
}
