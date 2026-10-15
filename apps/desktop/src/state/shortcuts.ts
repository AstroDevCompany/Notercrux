import { useSyncExternalStore } from 'react';

/**
 * Global shortcuts and their user-customizable bindings.
 *
 * Only these app-level commands are rebindable. Editor shortcuts (formatting, undo/redo, block
 * moves) follow platform conventions and stay fixed so muscle memory and Markdown habits keep
 * working; page-tree shortcuts depend on focus and are likewise fixed.
 */
export type CommandId = 'palette' | 'newPage' | 'toggleSidebar' | 'settings' | 'back' | 'forward';

export interface CommandDef {
  id: CommandId;
  label: string;
  /** Default binding, e.g. `Mod+K`. `Mod` is ⌘ on macOS and Ctrl elsewhere. */
  default: string;
}

export const COMMANDS: CommandDef[] = [
  { id: 'palette', label: 'Command palette', default: 'Mod+K' },
  { id: 'newPage', label: 'New page', default: 'Mod+N' },
  { id: 'toggleSidebar', label: 'Toggle sidebar', default: 'Mod+\\' },
  { id: 'settings', label: 'Open settings', default: 'Mod+,' },
  { id: 'back', label: 'Go back', default: 'Mod+[' },
  { id: 'forward', label: 'Go forward', default: 'Mod+]' },
];

const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
const KEY = 'notercrux.shortcuts.v1';

let overrides: Partial<Record<CommandId, string>> = (() => {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<Record<CommandId, string>>;
  } catch {
    return {};
  }
})();
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

export const bindingOf = (id: CommandId): string =>
  overrides[id] ?? COMMANDS.find((c) => c.id === id)!.default;

/** Stable snapshot string for useSyncExternalStore. */
const snapshot = () => JSON.stringify(overrides);

export function useBindings(): Record<CommandId, string> {
  useSyncExternalStore((cb) => (listeners.add(cb), () => listeners.delete(cb)), snapshot);
  return Object.fromEntries(COMMANDS.map((c) => [c.id, bindingOf(c.id)])) as Record<
    CommandId,
    string
  >;
}

/** Normalizes a keyboard event to a binding string like `Mod+Shift+K`, or null for bare modifiers. */
export function eventToBinding(e: KeyboardEvent): string | null {
  if (['Control', 'Meta', 'Shift', 'Alt'].includes(e.key)) return null;
  const mod = isMac ? e.metaKey : e.ctrlKey;
  // The "other" primary modifier is not part of the Mod abstraction; reject it so a binding means
  // the same thing on every platform.
  if (isMac ? e.ctrlKey : e.metaKey) return null;
  const parts: string[] = [];
  if (mod) parts.push('Mod');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  // With Alt held, macOS produces symbols (⌥K → ˚); use the physical key for letters and digits.
  let key = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  if (e.altKey && /^(Key[A-Z]|Digit\d)$/.test(e.code)) key = e.code.slice(-1);
  parts.push(key === ' ' ? 'Space' : key);
  return parts.join('+');
}

/** Which command (if any) another command already uses this binding. */
export function conflictOf(binding: string, except: CommandId): CommandDef | undefined {
  return COMMANDS.find((c) => c.id !== except && bindingOf(c.id) === binding);
}

/** A rebindable shortcut must include Mod or Alt, so typing text can never trigger a command. */
export const isValidBinding = (b: string) => b.split('+').some((p) => p === 'Mod' || p === 'Alt');

export function setBinding(id: CommandId, binding: string | null) {
  const next = { ...overrides };
  if (binding === null || binding === COMMANDS.find((c) => c.id === id)!.default) delete next[id];
  else next[id] = binding;
  overrides = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(overrides));
  } catch {
    /* ignore */
  }
  emit();
}

export function resetBindings() {
  overrides = {};
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* ignore */
  }
  emit();
}

export function commandForEvent(e: KeyboardEvent): CommandId | null {
  const b = eventToBinding(e);
  if (!b) return null;
  return COMMANDS.find((c) => bindingOf(c.id) === b)?.id ?? null;
}

/** Human-readable form: `⌘K` on macOS, `Ctrl+K` elsewhere. */
export function formatBinding(binding: string): string {
  const parts = binding.split('+');
  if (isMac) {
    const map: Record<string, string> = { Mod: '⌘', Alt: '⌥', Shift: '⇧' };
    return parts.map((p) => map[p] ?? p).join('');
  }
  return parts.map((p) => (p === 'Mod' ? 'Ctrl' : p)).join('+');
}
