import { useSyncExternalStore } from 'react';

export type ThemeSetting = 'system' | 'light' | 'dark';
/** `system` follows the OS "reduce transparency" preference. */
export type TransparencySetting = 'system' | 'glass' | 'reduced';
export type MotionSetting = 'system' | 'full' | 'reduced';

export interface Settings {
  theme: ThemeSetting;
  transparency: TransparencySetting;
  motion: MotionSetting;
}

const KEY = 'notercrux.settings.v1';
const DEFAULTS: Settings = { theme: 'system', transparency: 'system', motion: 'system' };

function read(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return { ...DEFAULTS, ...(JSON.parse(raw) as Partial<Settings>) };
  } catch {
    /* ignore: private mode or corrupted value */
  }
  return DEFAULTS;
}

let current = read();
const listeners = new Set<() => void>();

export function updateSettings(patch: Partial<Settings>) {
  current = { ...current, ...patch };
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    /* ignore */
  }
  listeners.forEach((l) => l());
}

export function useSettings(): Settings {
  return useSyncExternalStore(
    (cb) => (listeners.add(cb), () => listeners.delete(cb)),
    () => current,
  );
}
