import { beforeEach, describe, expect, it, vi } from 'vitest';

async function setup() {
  localStorage.clear();
  vi.resetModules();
  const app = await import('./app');
  await app.actions.init();
  const id = (title: string) => app.getState().pages.find((p) => p.title === title)!.id;
  const here = () => app.getState().pages.find((p) => p.id === app.getState().currentId)?.title;
  return { ...app, id, here };
}

describe('navigation history', () => {
  let t: Awaited<ReturnType<typeof setup>>;
  beforeEach(async () => {
    t = await setup();
  });

  it('goes back and forward, and visiting a new page drops the forward entries', () => {
    const { actions, id, here, getState, canGo } = t;
    expect(here()).toBe('Welcome to Notercrux');
    actions.open(id('Reading list'));
    actions.open(id('A nested page'));
    expect(canGo(getState(), 1)).toBe(false);
    actions.go(-1);
    expect(here()).toBe('Reading list');
    actions.go(-1);
    expect(here()).toBe('Welcome to Notercrux');
    expect(canGo(getState(), -1)).toBe(false);
    actions.go(1);
    expect(here()).toBe('Reading list');
    actions.open(id('Welcome to Notercrux')); // branch: forward entries are discarded
    expect(canGo(getState(), 1)).toBe(false);
    actions.go(-1);
    expect(here()).toBe('Reading list');
  });

  it('skips pages that were archived or deleted', async () => {
    const { actions, id, here } = t;
    actions.open(id('Reading list'));
    actions.open(id('A nested page'));
    await actions.archive(id('Reading list'));
    actions.go(-1); // Reading list is archived: lands on the first entry instead
    expect(here()).toBe('Welcome to Notercrux');
  });

  it('reveals a nested page by expanding its ancestors', () => {
    const { actions, id, getState } = t;
    actions.toggleExpanded(id('Welcome to Notercrux'), false);
    actions.open(id('A nested page'));
    expect(getState().expanded[id('Welcome to Notercrux')]).toBe(true);
  });
});
