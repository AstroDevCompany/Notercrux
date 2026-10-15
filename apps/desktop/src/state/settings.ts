import { useSyncExternalStore } from 'react';

export type ThemeSetting = 'system' | 'light' | 'dark';
/** `system` follows the OS "reduce transparency" preference. */
export type TransparencySetting = 'system' | 'glass' | 'reduced';
export type MotionSetting = 'system' | 'full' | 'reduced';

export type AccentSetting = 'mint' | 'indigo' | 'blue' | 'green' | 'pink' | 'orange' | 'graphite';
export type FontSetting = 'sans' | 'serif' | 'mono';
export type TextSizeSetting = 'small' | 'medium' | 'large';
export type WidthSetting = 'narrow' | 'default' | 'wide' | 'full';

export interface Settings {
  theme: ThemeSetting;
  transparency: TransparencySetting;
  motion: MotionSetting;
  accent: AccentSetting;
  /** Typeface of page content (the interface keeps the system font). */
  font: FontSetting;
  textSize: TextSizeSetting;
  pageWidth: WidthSetting;
}

const KEY = 'notercrux.settings.v1';
export const DEFAULTS: Settings = {
  theme: 'system',
  transparency: 'system',
  motion: 'system',
  accent: 'mint',
  font: 'sans',
  textSize: 'medium',
  pageWidth: 'default',
};

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
