import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Api, ApplyReport, SyncStatus } from './types';

export const tauriApi: Api = {
  listPages: () => invoke('list_pages'),
  createPage: (parentId, title) => invoke('create_page', { parentId, title }),
  renamePage: (pageId, title) => invoke('rename_page', { pageId, title }),
  setPageIcon: (pageId, icon) => invoke('set_page_icon', { pageId, icon }),
  movePage: (pageId, newParentId, index) => invoke('move_page', { pageId, newParentId, index }),
  duplicatePage: (pageId) => invoke('duplicate_page', { pageId }),
  archivePage: (pageId) => invoke('archive_page', { pageId }),
  restorePage: (pageId) => invoke('restore_page', { pageId }),
  deletePage: (pageId) => invoke('delete_page', { pageId }),
  loadDocument: (pageId) => invoke('load_document', { pageId }),
  saveDocument: (pageId, document) => invoke('save_document', { pageId, document }),
  saveDocumentRebased: (pageId, base, document) =>
    invoke('save_document_rebased', { pageId, base, document }),
  searchPages: (query) => invoke('search_pages', { query }),
  backlinks: (pageId) => invoke('backlinks', { pageId }),
  createDatabase: (parentId, title) => invoke('create_database', { parentId, title }),
  getSchema: (databaseId) => invoke('get_schema', { databaseId }),
  setSchema: (databaseId, schema) => invoke('set_schema', { databaseId, schema }),
  listRecords: (databaseId) => invoke('list_records', { databaseId }),
  setProperty: (pageId, propertyId, value) => invoke('set_property', { pageId, propertyId, value }),
  getProperties: (pageId) => invoke('get_properties', { pageId }),

  syncOverview: () => invoke('sync_overview'),
  syncEvents: () => invoke('sync_events'),
  syncCreateWorkspace: (server, token, label) =>
    invoke('sync_create_workspace', { server, token, label }),
  syncCreateInvite: () => invoke('sync_create_invite'),
  syncPendingJoin: (inviteId) => invoke('sync_pending_join', { inviteId }),
  syncApproveJoin: (inviteId) => invoke('sync_approve_join', { inviteId }),
  syncBeginJoin: (code, label) => invoke('sync_begin_join', { code, label }),
  syncPollJoin: (mode) => invoke('sync_poll_join', { mode }),
  syncCancelJoin: () => invoke('sync_cancel_join'),
  syncDevices: () => invoke('sync_devices'),
  syncInfo: () => invoke('sync_info'),
  syncRevokeDevice: (deviceId) => invoke('sync_revoke_device', { deviceId }),
  syncRecover: (server, workspaceId, phrase, label, mode) =>
    invoke('sync_recover', { server, workspaceId, phrase, label, mode }),
  syncLeave: () => invoke('sync_leave'),
  syncLeaveLocal: () => invoke('sync_leave_local'),
  syncDeleteRemote: () => invoke('sync_delete_remote'),
  syncNow: () => invoke('sync_now'),
  syncRetryAttachments: () => invoke('sync_retry_attachments'),
  async subscribeSync({ onStatus, onChanges }) {
    const offs = await Promise.all([
      listen<SyncStatus>('sync-status', (e) => onStatus?.(e.payload)),
      listen<ApplyReport>('sync-changes', (e) => onChanges?.(e.payload)),
    ]);
    return () => offs.forEach((off) => off());
  },

  // The file travels as the raw request body; name and type go in percent-encoded headers.
  addAttachment: ({ name, mime, bytes }) =>
    invoke('add_attachment', bytes, {
      headers: { 'x-name': encodeURIComponent(name), 'x-mime': encodeURIComponent(mime) },
    }),
  attachmentInfo: (id) => invoke('attachment_info', { id }),
  readAttachment: async (id) =>
    new Uint8Array(await invoke<ArrayBuffer>('read_attachment', { id })),
  saveAttachmentAs: (id) => invoke('save_attachment_as', { id }),
};
