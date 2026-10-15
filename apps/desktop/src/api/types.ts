import type { Document, Page } from '@notercrux/document-model';

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
  searchPages(query: string): Promise<Page[]>;
}
