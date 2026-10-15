import { mockApi } from './mock';
import { tauriApi } from './tauri';
import type { Api } from './types';

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const api: Api = isTauri ? tauriApi : mockApi;
export type { Api, SearchHit } from './types';
