import type {
  Api,
  AttachmentMeta,
  DeviceView,
  JoinMode,
  SyncEvent,
  SyncHandlers,
  SyncOverview,
  SyncStatus,
} from './types';

/**
 * In-memory stand-in for the sync engine, used in the browser preview and in unit tests. It follows
 * the same flows as the native engine (create, invite, approve, join, revoke, recover) but talks to
 * no server and encrypts nothing. The Rust `notercrux-sync` crate is the source of truth.
 */
const idle = (): SyncStatus => ({
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
});

const B32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
const randomPhrase = () =>
  Array.from({ length: 14 }, () =>
    Array.from({ length: 4 }, () => B32[Math.floor(Math.random() * 32)]).join(''),
  ).join('-');
const rid = (n: number) =>
  Array.from({ length: n }, () => Math.floor(Math.random() * 16).toString(16)).join('');

let status: SyncStatus = idle();
let devices: DeviceView[] = [];
let events: SyncEvent[] = [];
const handlers = new Set<SyncHandlers>();
let invite: { id: string; polled: number } | null = null;
let pendingJoin: { label: string; polls: number } | null = null;

function setStatus(patch: Partial<SyncStatus>) {
  status = { ...status, ...patch };
  handlers.forEach((h) => h.onStatus?.(status));
}

function log(level: SyncEvent['level'], message: string) {
  events = [...events, { at: Math.floor(Date.now() / 1000), level, message }].slice(-100);
}

function link(server: string, label: string) {
  const deviceId = rid(32);
  devices = [
    {
      id: deviceId,
      label,
      current: true,
      revoked: false,
      createdAt: Date.now() / 1000,
      lastSeen: null,
    },
  ];
  setStatus({
    phase: 'idle',
    server,
    workspaceId: `w${rid(32)}`,
    deviceId,
    lastSyncAt: Math.floor(Date.now() / 1000),
  });
  log('info', 'Connected to the sync server (browser preview, nothing is sent anywhere).');
}

function needServer(server: string) {
  if (!/^https?:\/\/.{3,}/.test(server.trim())) {
    throw new Error(
      'The server address must start with https:// (or http:// on a trusted network).',
    );
  }
}

function needLinked() {
  if (status.phase === 'disabled')
    throw new Error('This workspace is not connected to a sync server');
}

export function resetMockSync() {
  status = idle();
  devices = [];
  events = [];
  invite = null;
  pendingJoin = null;
}

/** Test and preview helper: pretend something happened on the sync engine. */
export function mockSyncPatch(patch: Partial<SyncStatus>) {
  setStatus(patch);
}

const attachments = new Map<string, { meta: AttachmentMeta; bytes: Uint8Array }>();

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  try {
    const digest = await crypto.subtle.digest('SHA-256', bytes as BufferSource);
    return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, '0')).join('');
  } catch {
    // No WebCrypto (very old test environments): a stable stand-in of the right shape.
    let h = 0x811c9dc5;
    for (const b of bytes) h = Math.imul(h ^ b, 16777619) >>> 0;
    return h.toString(16).padStart(8, '0').repeat(8);
  }
}

type SyncPart = Pick<
  Api,
  | 'syncOverview'
  | 'syncEvents'
  | 'syncCreateWorkspace'
  | 'syncCreateInvite'
  | 'syncPendingJoin'
  | 'syncApproveJoin'
  | 'syncBeginJoin'
  | 'syncPollJoin'
  | 'syncCancelJoin'
  | 'syncDevices'
  | 'syncInfo'
  | 'syncRevokeDevice'
  | 'syncRecover'
  | 'syncLeave'
  | 'syncLeaveLocal'
  | 'syncDeleteRemote'
  | 'syncNow'
  | 'syncRetryAttachments'
  | 'subscribeSync'
  | 'addAttachment'
  | 'attachmentInfo'
  | 'readAttachment'
  | 'saveAttachmentAs'
>;

export const mockSyncApi: SyncPart = {
  async syncOverview(): Promise<SyncOverview> {
    return { status, secureStorage: null, joinInProgress: pendingJoin !== null };
  },
  async syncEvents() {
    return events;
  },
  async syncCreateWorkspace(server, _token, label) {
    if (status.phase !== 'disabled') throw new Error('This workspace is already connected.');
    needServer(server);
    link(server.trim().replace(/\/+$/, ''), label || 'This device');
    return { workspaceId: status.workspaceId!, recoveryPhrase: randomPhrase() };
  },
  async syncCreateInvite() {
    needLinked();
    invite = { id: rid(32), polled: 0 };
    return {
      inviteId: invite.id,
      code: `NCX1-${rid(60)}`,
      expiresAt: Math.floor(Date.now() / 1000) + 900,
    };
  },
  async syncPendingJoin(inviteId) {
    if (!invite || invite.id !== inviteId) return null;
    invite.polled += 1;
    // In the preview, a "phone" asks to join on the second look.
    if (invite.polled < 2) return null;
    return { deviceId: rid(32), label: 'Demo phone', sas: '4821', verified: true };
  },
  async syncApproveJoin(inviteId) {
    needLinked();
    if (!invite || invite.id !== inviteId) throw new Error('That invitation is no longer valid.');
    devices = [
      ...devices,
      {
        id: rid(32),
        label: 'Demo phone',
        current: false,
        revoked: false,
        createdAt: Date.now() / 1000,
        lastSeen: null,
      },
    ];
    invite = null;
    log('info', 'Added device "Demo phone".');
  },
  async syncBeginJoin(code, label) {
    if (status.phase !== 'disabled') throw new Error('This workspace is already connected.');
    if (!/^NCX1-[A-Za-z0-9_-]{8,}$/.test(code.trim())) throw new Error('That code is not valid');
    pendingJoin = { label: label || 'This device', polls: 0 };
    return '4821';
  },
  async syncPollJoin(_mode: JoinMode) {
    if (!pendingJoin) throw new Error('No join is in progress.');
    pendingJoin.polls += 1;
    if (pendingJoin.polls < 2) return 'waiting';
    link('https://sync.example.org', pendingJoin.label);
    pendingJoin = null;
    return 'joined';
  },
  async syncCancelJoin() {
    pendingJoin = null;
  },
  async syncDevices() {
    needLinked();
    return devices;
  },
  async syncInfo() {
    needLinked();
    return {
      workspaceId: status.workspaceId!,
      epoch: status.epoch,
      usage: {
        opBytes: 48_213,
        opCount: 312,
        blobBytes: 0,
        devices: devices.filter((d) => !d.revoked).length,
      },
      limits: {
        maxDevices: 10,
        maxOpBytesTotal: 2 * 1024 ** 3,
        maxBlobBytesTotal: 10 * 1024 ** 3,
        maxOpSize: 4 * 1024 ** 2,
        maxBlobSize: 512 * 1024 ** 2,
      },
    };
  },
  async syncRevokeDevice(deviceId) {
    needLinked();
    const target = devices.find((d) => d.id === deviceId);
    if (!target) throw new Error('not found');
    if (devices.filter((d) => !d.revoked).length < 2)
      throw new Error('cannot revoke the last device');
    devices = devices.map((d) => (d.id === deviceId ? { ...d, revoked: true } : d));
    setStatus({ epoch: status.epoch + 1 });
    log('info', 'Device removed and the workspace key rotated.');
  },
  async syncRecover(server, workspaceId, phrase, label) {
    if (status.phase !== 'disabled') throw new Error('This workspace is already connected.');
    needServer(server);
    if (!/^([A-Z2-7]{4}-){13}[A-Z2-7]{4}$/i.test(phrase.trim().replace(/\s+/g, '-'))) {
      throw new Error('That code is not valid');
    }
    link(server.trim(), label || 'This device');
    setStatus({ workspaceId });
  },
  async syncLeave() {
    resetMockSync();
    handlers.forEach((h) => h.onStatus?.(status));
  },
  async syncLeaveLocal() {
    resetMockSync();
    handlers.forEach((h) => h.onStatus?.(status));
  },
  async syncDeleteRemote() {
    resetMockSync();
    handlers.forEach((h) => h.onStatus?.(status));
  },
  async syncNow() {
    if (status.phase !== 'disabled') setStatus({ lastSyncAt: Math.floor(Date.now() / 1000) });
  },
  async syncRetryAttachments() {
    setStatus({ attachmentsFailed: 0 });
    return 0;
  },
  async subscribeSync(h) {
    handlers.add(h);
    return () => {
      handlers.delete(h);
    };
  },

  async addAttachment({ name, mime, bytes }) {
    if (!bytes.length) throw new Error('the file is empty');
    if (bytes.length > 100 * 1024 * 1024)
      throw new Error('files larger than 100 MB are not supported');
    const id = await sha256Hex(bytes);
    const meta: AttachmentMeta = {
      id,
      name: name.split(/[\\/]/).pop() || 'attachment',
      mime: mime || 'application/octet-stream',
      size: bytes.length,
      state: 'local',
      error: null,
      chunksPresent: Math.max(1, Math.ceil(bytes.length / (1024 * 1024))),
      chunksTotal: Math.max(1, Math.ceil(bytes.length / (1024 * 1024))),
    };
    attachments.set(id, { meta, bytes: bytes.slice() });
    return meta;
  },
  async attachmentInfo(id) {
    return attachments.get(id)?.meta ?? null;
  },
  async readAttachment(id) {
    const a = attachments.get(id);
    if (!a) throw new Error('this file has not been downloaded yet');
    return a.bytes.slice();
  },
  async saveAttachmentAs(id) {
    if (!attachments.has(id)) throw new Error('this file has not been downloaded yet');
    return true;
  },
};
