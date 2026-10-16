import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';

beforeEach(() => {
  localStorage.clear();
  Element.prototype.scrollIntoView = vi.fn();
  window.matchMedia = ((q: string) => ({
    matches: false,
    media: q,
    addEventListener: () => {},
    removeEventListener: () => {},
  })) as unknown as typeof window.matchMedia;
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

async function boot() {
  vi.resetModules();
  const { default: App } = await import('../App');
  const app = await import('../state/app');
  const sync = await import('../state/sync');
  const { resetMockSync } = await import('../api/mock.sync');
  resetMockSync();
  render(<App />);
  await screen.findByRole('tree', { name: 'Pages' });
  return { app, sync };
}

async function openSettings(app: Awaited<ReturnType<typeof boot>>['app']) {
  act(() => app.actions.setSettings(true));
  return screen.findByRole('dialog', { name: 'Settings' });
}

describe('Sync settings and setup', () => {
  it('offers setup while sync is off and hides the status badge', async () => {
    const { app } = await boot();
    expect(screen.queryByRole('button', { name: /^Sync:/ })).toBeNull();
    const dlg = await openSettings(app);
    expect(dlg.textContent).toMatch(/End-to-end encrypted/);
    expect(screen.getByRole('button', { name: 'Set up…' })).toBeTruthy();
  });

  it('creates a workspace, forces the recovery key to be acknowledged, then shows the devices', async () => {
    const { app } = await boot();
    await openSettings(app);
    fireEvent.click(screen.getByRole('button', { name: 'Set up…' }));
    const wizard = await screen.findByRole('dialog', { name: 'Set up sync' });
    expect(wizard).toBeTruthy();
    const create = screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement;
    expect(create.disabled).toBe(true);

    fireEvent.change(screen.getByPlaceholderText('https://sync.example.org'), {
      target: { value: 'not a url' },
    });
    fireEvent.click(create);
    expect((await screen.findByRole('alert')).textContent).toMatch(/must start with https/);

    fireEvent.change(screen.getByPlaceholderText('https://sync.example.org'), {
      target: { value: 'https://sync.example.org' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    // The recovery key is shown once and Done stays disabled until the person confirms.
    const kit = await screen.findByLabelText('Recovery key');
    expect(kit.querySelectorAll('code')).toHaveLength(14);
    const done = screen.getByRole('button', { name: 'Done' }) as HTMLButtonElement;
    expect(done.disabled).toBe(true);
    fireEvent.click(screen.getByRole('checkbox'));
    expect(done.disabled).toBe(false);
    fireEvent.click(done);
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Set up sync' })).toBeNull());

    // Sync is now on: the sidebar shows its status and settings list this device.
    expect(await screen.findByRole('button', { name: /^Sync: Up to date/ })).toBeTruthy();
    await openSettings(app);
    expect(await screen.findByText('This device')).toBeTruthy();
    expect(screen.getByRole('button', { name: /Sync now/ })).toBeTruthy();
  });

  it('adds a device: shows a code, asks for approval with the verification code, then lists it', async () => {
    const { app, sync } = await boot();
    const { api } = await import('../api');
    await api.syncCreateWorkspace('https://sync.example.org', null, 'Laptop');
    await sync.refreshSync();
    await openSettings(app);
    fireEvent.click(await screen.findByRole('button', { name: 'Add device' }));
    expect(await screen.findByRole('dialog', { name: 'Add a device' })).toBeTruthy();
    const code = (await screen.findByLabelText('Invite code')) as HTMLTextAreaElement;
    await waitFor(() => expect(code.value).toMatch(/^NCX1-/));

    // The new device asks to join (the mock does so on its second look).
    const approve = await screen.findByRole('button', { name: 'Approve' }, { timeout: 6000 });
    expect(screen.getByLabelText('Verification code').textContent).toBe('4821');
    fireEvent.click(approve);
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Add a device' })).toBeNull());
    expect(await screen.findByText(/Added “Demo phone”/)).toBeTruthy();
    expect((await api.syncDevices()).map((d) => d.label)).toContain('Demo phone');
  }, 15000);

  it('removes another device only after confirmation, and reports key rotation', async () => {
    const { app, sync } = await boot();
    const { api } = await import('../api');
    await api.syncCreateWorkspace('https://sync.example.org', null, 'Laptop');
    const offer = await api.syncCreateInvite();
    await api.syncPendingJoin(offer.inviteId);
    await api.syncPendingJoin(offer.inviteId);
    await api.syncApproveJoin(offer.inviteId);
    await sync.refreshSync();
    await openSettings(app);
    const row = (await screen.findByText('Demo phone')).closest('li')!;
    fireEvent.click(row.querySelector('button')!);
    const confirm = await screen.findByRole('dialog', { name: /Remove “Demo phone”/ });
    expect(confirm.textContent).toMatch(/new encryption key/);
    const { within } = await import('@testing-library/react');
    fireEvent.click(within(confirm).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(screen.queryByText('Demo phone')).toBeNull());
    expect(sync.getSync().status.epoch).toBe(1);
  });

  it('shows the join code flow with the verification code and keeps polling until approved', async () => {
    const { app } = await boot();
    await openSettings(app);
    fireEvent.click(screen.getByRole('button', { name: 'Set up…' }));
    fireEvent.click(await screen.findByRole('tab', { name: 'Join with a code' }));
    fireEvent.change(screen.getByPlaceholderText('NCX1-…'), { target: { value: 'garbage' } });
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/not valid/);

    fireEvent.change(screen.getByPlaceholderText('NCX1-…'), {
      target: { value: 'NCX1-abcdefghijklmnop' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    expect((await screen.findByLabelText('Verification code')).textContent).toBe('4821');
    expect(screen.getByText(/Waiting for approval/)).toBeTruthy();
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Set up sync' })).toBeNull(), {
      timeout: 6000,
    });
    expect(await screen.findByRole('button', { name: /^Sync:/ })).toBeTruthy();
  }, 15000);

  it('warns when a join request did not use the invite code', async () => {
    const { app, sync } = await boot();
    const { api } = await import('../api');
    await api.syncCreateWorkspace('https://sync.example.org', null, 'Laptop');
    await sync.refreshSync();
    const spy = vi
      .spyOn(api, 'syncPendingJoin')
      .mockResolvedValue({ deviceId: 'x', label: 'Unknown', sas: '0000', verified: false });
    await openSettings(app);
    fireEvent.click(await screen.findByRole('button', { name: 'Add device' }));
    const approve = await screen.findByRole('button', { name: 'Approve' }, { timeout: 6000 });
    expect((approve as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole('alert').textContent).toMatch(/Do not approve/);
    spy.mockRestore();
  }, 15000);

  it('tells a removed device what happened and offers to keep its pages', async () => {
    const { app, sync } = await boot();
    const { api } = await import('../api');
    const { mockSyncPatch } = await import('../api/mock.sync');
    await api.syncCreateWorkspace('https://sync.example.org', null, 'Laptop');
    mockSyncPatch({ phase: 'revoked', lastError: 'This device was removed from the workspace' });
    await sync.refreshSync();
    await openSettings(app);
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toMatch(/removed from the workspace/);
    fireEvent.click(screen.getByRole('button', { name: 'Keep my pages and disconnect' }));
    await waitFor(() => expect(sync.getSync().status.phase).toBe('disabled'));
  });
});

describe('Remote changes', () => {
  it('refreshes the page list and tells the person when an edit was restored', async () => {
    const { app, sync } = await boot();
    const { api } = await import('../api');
    const before = app.getState().pages.length;
    await api.createPage(null, 'Created elsewhere');
    sync.handleRemoteChanges({
      applied: 1,
      held: 0,
      recoveredBlocks: 2,
      pagesChanged: true,
      documentsChanged: ['ws'],
    });
    await waitFor(() => expect(app.getState().pages.length).toBe(before + 1));
    expect(app.getState().notice?.text).toMatch(/Another device deleted 2 blocks/);
  });
});
