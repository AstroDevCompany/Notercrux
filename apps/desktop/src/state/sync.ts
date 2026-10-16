import { useSyncExternalStore } from 'react';
import { api } from '../api';
import type { ApplyReport, SyncEvent, SyncOverview, SyncStatus } from '../api/types';
import { actions } from './app';
import { announceRemoteChange } from './editorBridge';

const DISABLED: SyncStatus = {
  phase: 'disabled',
  server: null,
  workspaceId: null,
  deviceId: null,
  epoch: 0,
  pendingOps: 0,
  heldOps: 0,
  lastSyncAt: null,
  lastError: null,
  retryInSecs: null,
  opsSent: 0,
  opsReceived: 0,
  rejectedOps: 0,
  recoveredBlocks: 0,
  attachmentsPending: 0,
  attachmentsFailed: 0,
};

export type SyncDialog = 'wizard' | 'addDevice' | null;

interface SyncState {
  dialog: SyncDialog;
  status: SyncStatus;
  secureStorage: SyncOverview['secureStorage'];
  joinInProgress: boolean;
  events: SyncEvent[];
}

let state: SyncState = {
  dialog: null,
  status: DISABLED,
  secureStorage: null,
  joinInProgress: false,
  events: [],
};
const listeners = new Set<() => void>();

function set(patch: Partial<SyncState>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

export function useSync<T>(select: (s: SyncState) => T): T {
  return useSyncExternalStore(
    (cb) => (listeners.add(cb), () => listeners.delete(cb)),
    () => select(state),
  );
}
export const getSync = () => state;

/** Sync dialogs sit beside Settings, so opening one closes Settings. */
export function openSyncDialog(dialog: Exclude<SyncDialog, null>) {
  actions.setSettings(false);
  set({ dialog });
}
export const closeSyncDialog = () => set({ dialog: null });

/** Re-reads the engine's state (after a user action, and when the settings dialog opens). */
export async function refreshSync() {
  try {
    const [overview, events] = await Promise.all([api.syncOverview(), api.syncEvents()]);
    set({
      status: overview.status,
      secureStorage: overview.secureStorage,
      joinInProgress: overview.joinInProgress,
      events,
    });
  } catch {
    /* the engine is not available (plain browser without the mock); stay disabled */
  }
}

/** What to do when the engine applied other devices' changes locally. */
export function handleRemoteChanges(r: ApplyReport) {
  if (r.pagesChanged) void actions.refresh();
  announceRemoteChange(r.documentsChanged);
  if (r.recoveredBlocks > 0) {
    actions.notify(
      `Another device deleted ${r.recoveredBlocks === 1 ? 'a block' : `${r.recoveredBlocks} blocks`} you were editing.`,
      [
        'Your text was restored in a callout at the end of the page, so nothing was lost.',
        'You can move it back or delete it.',
      ],
    );
  }
}

let started = false;
/** Subscribes to the engine once for the life of the app. */
export async function startSync() {
  if (started) return;
  started = true;
  await refreshSync();
  await api.subscribeSync({
    onStatus: (status) => set({ status }),
    onChanges: handleRemoteChanges,
  });
}

/** Time since an instant, in words. */
export function ago(unixSecs: number | null, now = Date.now()): string {
  if (!unixSecs) return 'never';
  const s = Math.max(0, Math.round(now / 1000 - unixSecs));
  if (s < 10) return 'just now';
  if (s < 60) return `${s} seconds ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
}

export const PHASE_LABEL: Record<SyncStatus['phase'], string> = {
  disabled: 'Sync is off',
  idle: 'Up to date',
  syncing: 'Syncing',
  offline: 'Offline, will retry',
  error: 'Sync problem',
  revoked: 'Removed from workspace',
  keysMissing: 'Keys missing',
};

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 10 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}
