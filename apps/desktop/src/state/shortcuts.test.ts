import { beforeEach, describe, expect, it, vi } from 'vitest';

async function load() {
  localStorage.clear();
  vi.resetModules();
  return import('./shortcuts');
}

// jsdom is not a Mac, so Mod = Ctrl.
const key = (k: string, init: KeyboardEventInit = {}, code = '') =>
  new KeyboardEvent('keydown', { key: k, code, ...init });

describe('keyboard shortcut registry', () => {
  let sc: Awaited<ReturnType<typeof load>>;
  beforeEach(async () => {
    sc = await load();
  });

  it('maps key events to bindings and to default commands', () => {
    expect(sc.eventToBinding(key('k', { ctrlKey: true }))).toBe('Mod+K');
    expect(sc.eventToBinding(key('K', { ctrlKey: true, shiftKey: true }))).toBe('Mod+Shift+K');
    expect(sc.eventToBinding(key('Control', { ctrlKey: true }))).toBeNull(); // bare modifier
    expect(sc.eventToBinding(key('k', { metaKey: true }))).toBeNull(); // the "other" primary key
    expect(sc.eventToBinding(key('˚', { ctrlKey: true, altKey: true }, 'KeyK'))).toBe('Mod+Alt+K');
    expect(sc.commandForEvent(key('k', { ctrlKey: true }))).toBe('palette');
    expect(sc.commandForEvent(key('[', { ctrlKey: true }))).toBe('back');
    expect(sc.commandForEvent(key('k'))).toBeNull(); // typing never triggers commands
  });

  it('rebinds, persists, detects conflicts and resets', async () => {
    sc.setBinding('newPage', 'Mod+Shift+N');
    expect(sc.commandForEvent(key('N', { ctrlKey: true, shiftKey: true }))).toBe('newPage');
    expect(sc.commandForEvent(key('n', { ctrlKey: true }))).toBeNull(); // old default is free now
    expect(sc.conflictOf('Mod+K', 'newPage')?.id).toBe('palette');
    expect(sc.conflictOf('Mod+Shift+N', 'newPage')).toBeUndefined(); // own binding is not a clash

    // Survives a reload.
    vi.resetModules();
    const again = await import('./shortcuts');
    expect(again.bindingOf('newPage')).toBe('Mod+Shift+N');

    again.setBinding('newPage', null);
    expect(again.bindingOf('newPage')).toBe('Mod+N');
    again.setBinding('palette', 'Alt+P');
    again.resetBindings();
    expect(again.bindingOf('palette')).toBe('Mod+K');
  });

  it('only accepts bindings that cannot be triggered by plain typing', () => {
    expect(sc.isValidBinding('Mod+K')).toBe(true);
    expect(sc.isValidBinding('Alt+P')).toBe(true);
    expect(sc.isValidBinding('Shift+K')).toBe(false);
    expect(sc.isValidBinding('K')).toBe(false);
  });

  it('formats bindings for the platform', () => {
    expect(sc.formatBinding('Mod+Shift+K')).toBe('Ctrl+Shift+K');
  });
});
