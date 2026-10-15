import { invoke } from '@tauri-apps/api/core';
import type { Api } from './types';

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
  searchPages: (query) => invoke('search_pages', { query }),
};
