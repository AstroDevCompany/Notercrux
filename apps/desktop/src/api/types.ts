import type { DatabaseSchema, DbRecord, Document, Page } from '@notercrux/document-model';

export interface SearchHit {
  page: Page;
  /** Excerpt with matches wrapped in U+0001 … U+0002. */
  snippet: string;
}

export type SyncPhase =
  'disabled' | 'idle' | 'syncing' | 'offline' | 'error' | 'revoked' | 'keysMissing';

export interface SyncStatus {
  phase: SyncPhase;
  server: string | null;
  workspaceId: string | null;
  deviceId: string | null;
  epoch: number;
  pendingOps: number;
  heldOps: number;
  lastSyncAt: number | null;
  lastError: string | null;
  retryInSecs: number | null;
  opsSent: number;
  opsReceived: number;
  rejectedOps: number;
  recoveredBlocks: number;
  attachmentsPending: number;
  attachmentsFailed: number;
}

export interface SyncOverview {
  status: SyncStatus;
  /** Where device keys are kept: the OS secure storage, a private file, or nowhere (null until first use). */
  secureStorage: 'os' | 'file' | 'none' | null;
  joinInProgress: boolean;
}

export interface SyncEvent {
  at: number;
  level: 'info' | 'warn' | 'error';
  message: string;
}

export interface CreatedWorkspace {
  workspaceId: string;
  /** Shown once. The only way back in if every device is lost. */
  recoveryPhrase: string;
}

export interface DeviceView {
  id: string;
  label: string;
  current: boolean;
  revoked: boolean;
  createdAt: number;
  lastSeen: number | null;
}

export interface InviteOffer {
  inviteId: string;
  code: string;
  expiresAt: number;
}

export interface JoinRequest {
  deviceId: string;
  label: string;
  /** Short code both screens show; the person checks they match. */
  sas: string;
  verified: boolean;
}

export type JoinMode = 'keepLocal' | 'replaceLocal';
export type JoinProgress = 'waiting' | 'joined';

export interface WorkspaceInfo {
  workspaceId: string;
  epoch: number;
  usage: { opBytes: number; opCount: number; blobBytes: number; devices: number };
  limits: {
    maxDevices: number;
    maxOpBytesTotal: number;
    maxBlobBytesTotal: number;
    maxOpSize: number;
    maxBlobSize: number;
  };
}

export interface ApplyReport {
  applied: number;
  held: number;
  recoveredBlocks: number;
  pagesChanged: boolean;
  /** Document keys: `page:<id>` or `ws`. */
  documentsChanged: string[];
}

export type AttachmentState = 'local' | 'synced' | 'missing' | 'failed';

export interface AttachmentMeta {
  id: string;
  name: string;
  mime: string;
  size: number;
  state: AttachmentState;
  error: string | null;
  chunksPresent: number;
  chunksTotal: number;
}

export interface Rebased {
  document: Document;
  /** Blocks edited on both devices in the same place (the other version went to a callout). */
  conflicts: number;
}

export interface SyncHandlers {
  onStatus?: (s: SyncStatus) => void;
  onChanges?: (r: ApplyReport) => void;
}

/** Everything the UI needs from the backend. Implemented by Tauri (SQLite) and by an in-browser mock. */
export interface Api {
  listPages(): Promise<Page[]>;
  createPage(parentId: string | null, title: string): Promise<Page>;
  renamePage(id: string, title: string): Promise<void>;
  setPageIcon(id: string, icon: string | null): Promise<void>;
  movePage(id: string, newParentId: string | null, index: number): Promise<void>;
  duplicatePage(id: string): Promise<Page>;
  archivePage(id: string): Promise<void>;
  restorePage(id: string): Promise<void>;
  deletePage(id: string): Promise<void>;
  loadDocument(id: string): Promise<Document>;
  saveDocument(id: string, doc: Document): Promise<void>;
  /**
   * Saves the editor's changes on top of the page as it is now. `base` is the copy the editor last
   * matched; changes other devices made since are kept. Returns the merged page.
   */
  saveDocumentRebased(id: string, base: Document, doc: Document): Promise<Rebased>;
  searchPages(query: string): Promise<SearchHit[]>;
  backlinks(id: string): Promise<Page[]>;

  createDatabase(parentId: string | null, title: string): Promise<Page>;
  getSchema(databaseId: string): Promise<DatabaseSchema>;
  setSchema(databaseId: string, schema: DatabaseSchema): Promise<void>;
  listRecords(databaseId: string): Promise<DbRecord[]>;
  /** `null` clears the value. */
  setProperty(pageId: string, propertyId: string, value: unknown): Promise<void>;
  getProperties(pageId: string): Promise<Record<string, unknown>>;

  // ---- sync ----
  syncOverview(): Promise<SyncOverview>;
  syncEvents(): Promise<SyncEvent[]>;
  syncCreateWorkspace(
    server: string,
    token: string | null,
    label: string,
  ): Promise<CreatedWorkspace>;
  syncCreateInvite(): Promise<InviteOffer>;
  syncPendingJoin(inviteId: string): Promise<JoinRequest | null>;
  syncApproveJoin(inviteId: string): Promise<void>;
  /** Returns the short code to compare with the other device. */
  syncBeginJoin(code: string, label: string): Promise<string>;
  syncPollJoin(mode: JoinMode): Promise<JoinProgress>;
  syncCancelJoin(): Promise<void>;
  syncDevices(): Promise<DeviceView[]>;
  syncInfo(): Promise<WorkspaceInfo>;
  syncRevokeDevice(deviceId: string): Promise<void>;
  syncRecover(
    server: string,
    workspaceId: string,
    phrase: string,
    label: string,
    mode: JoinMode,
  ): Promise<void>;
  syncLeave(): Promise<void>;
  syncLeaveLocal(): Promise<void>;
  syncDeleteRemote(): Promise<void>;
  syncNow(): Promise<void>;
  syncRetryAttachments(): Promise<number>;
  /** Live updates from the sync engine. Resolves to an unsubscribe function. */
  subscribeSync(handlers: SyncHandlers): Promise<() => void>;

  // ---- attachments ----
  addAttachment(file: { name: string; mime: string; bytes: Uint8Array }): Promise<AttachmentMeta>;
  attachmentInfo(id: string): Promise<AttachmentMeta | null>;
  /** The whole file. Rejects while it has not been downloaded yet. */
  readAttachment(id: string): Promise<Uint8Array>;
  /** Asks where to save; resolves false if cancelled. */
  saveAttachmentAs(id: string): Promise<boolean>;
}
