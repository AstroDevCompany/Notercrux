import type { DatabaseSchema, DbRecord, Document, Page } from '@notercrux/document-model';

export interface SearchHit {
  page: Page;
  /** Excerpt with matches wrapped in U+0001 … U+0002. */
  snippet: string;
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
  searchPages(query: string): Promise<SearchHit[]>;
  backlinks(id: string): Promise<Page[]>;

  createDatabase(parentId: string | null, title: string): Promise<Page>;
  getSchema(databaseId: string): Promise<DatabaseSchema>;
  setSchema(databaseId: string, schema: DatabaseSchema): Promise<void>;
  listRecords(databaseId: string): Promise<DbRecord[]>;
  /** `null` clears the value. */
  setProperty(pageId: string, propertyId: string, value: unknown): Promise<void>;
  getProperties(pageId: string): Promise<Record<string, unknown>>;
}
