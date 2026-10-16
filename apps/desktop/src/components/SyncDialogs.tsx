import { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import type { InviteOffer, JoinMode, JoinRequest } from '../api/types';
import { saveTextFile } from '../lib/files';
import { actions } from '../state/app';
import { closeSyncDialog, refreshSync, useSync } from '../state/sync';
import { Dialog } from './Dialog';

type Mode = 'create' | 'join' | 'recover';

const deviceName = () =>
  /Mac/i.test(navigator.platform)
    ? 'My Mac'
    : /Win/i.test(navigator.platform)
      ? 'My Windows PC'
      : 'My Linux PC';

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint && <span className="field-hint">{hint}</span>}
    </label>
  );
}

function LocalChoice({ value, onChange }: { value: JoinMode; onChange: (m: JoinMode) => void }) {
  return (
    <fieldset className="choice">
      <legend>The pages already on this device</legend>
      <label>
        <input
          type="radio"
          name="local-mode"
          checked={value === 'keepLocal'}
          onChange={() => onChange('keepLocal')}
        />
        <span>
          <strong>Keep them</strong> and add them to the workspace
        </span>
      </label>
      <label>
        <input
          type="radio"
          name="local-mode"
          checked={value === 'replaceLocal'}
          onChange={() => onChange('replaceLocal')}
        />
        <span>
          <strong>Replace them</strong> with the workspace&apos;s pages (deletes the pages on this
          device)
        </span>
      </label>
    </fieldset>
  );
}

function RecoveryKit({
  phrase,
  workspaceId,
  server,
  onDone,
}: {
  phrase: string;
  workspaceId: string;
  server: string;
  onDone: () => void;
}) {
  const [saved, setSaved] = useState(false);
  const [copied, setCopied] = useState(false);
  const text = `Notercrux recovery key\n\nServer: ${server}\nWorkspace: ${workspaceId}\nRecovery key: ${phrase}\n\nKeep this somewhere safe and private. Anyone with the key can read your notes; without it (and without another signed-in device) your notes cannot be recovered.\n`;
  return (
    <>
      <p className="dialog-body">
        Your notes are encrypted on your devices. This recovery key is the only way back in if you
        lose every device. <strong>It is shown only once and cannot be reset.</strong>
      </p>
      <div className="kit" aria-label="Recovery key">
        {phrase.split('-').map((g, i) => (
          <code key={i}>{g}</code>
        ))}
      </div>
      <div className="kit-actions">
        <button
          className="btn"
          onClick={() => {
            void navigator.clipboard
              ?.writeText(phrase)
              .then(() => setCopied(true))
              .catch(() => {});
          }}
        >
          {copied ? 'Copied' : 'Copy'}
        </button>
        <button
          className="btn"
          onClick={() => void saveTextFile('Notercrux recovery key', 'txt', text)}
        >
          Save as file
        </button>
      </div>
      <label className="confirm-line">
        <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
        <span>I stored the recovery key somewhere safe</span>
      </label>
      <div className="dialog-actions">
        <button className="btn btn-primary" disabled={!saved} onClick={onDone}>
          Done
        </button>
      </div>
    </>
  );
}

function WizardBody() {
  const [mode, setMode] = useState<Mode>('create');
  const [server, setServer] = useState('');
  const [token, setToken] = useState('');
  const [label, setLabel] = useState(deviceName());
  const [code, setCode] = useState('');
  const [workspaceId, setWorkspaceId] = useState('');
  const [phrase, setPhrase] = useState('');
  const [local, setLocal] = useState<JoinMode>('keepLocal');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [kit, setKit] = useState<{ phrase: string; workspaceId: string } | null>(null);
  const [sas, setSas] = useState<string | null>(null);

  // Joining: wait for the other device to approve.
  useEffect(() => {
    if (!sas) return;
    let stop = false;
    const tick = async () => {
      try {
        const p = await api.syncPollJoin(local);
        if (stop) return;
        if (p === 'joined') {
          await refreshSync();
          await actions.refresh();
          closeSyncDialog();
        }
      } catch (e) {
        if (!stop) {
          setError(message(e));
          setSas(null);
        }
      }
    };
    const t = setInterval(() => void tick(), 2000);
    return () => {
      stop = true;
      clearInterval(t);
    };
  }, [sas, local]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  if (kit) {
    return (
      <RecoveryKit
        phrase={kit.phrase}
        workspaceId={kit.workspaceId}
        server={server}
        onDone={closeSyncDialog}
      />
    );
  }

  if (sas) {
    return (
      <>
        <p className="dialog-body">
          Open Notercrux on your other device and approve this request. Check that it shows the same
          code:
        </p>
        <div className="sas" aria-label="Verification code">
          {sas}
        </div>
        <p className="dialog-body" role="status">
          Waiting for approval…
        </p>
        <div className="dialog-actions">
          <button
            className="btn"
            onClick={() => {
              void api.syncCancelJoin();
              setSas(null);
            }}
          >
            Cancel
          </button>
        </div>
      </>
    );
  }

  const submit = () =>
    run(async () => {
      if (mode === 'create') {
        const r = await api.syncCreateWorkspace(server, token.trim() || null, label);
        await refreshSync();
        setKit({ phrase: r.recoveryPhrase, workspaceId: r.workspaceId });
      } else if (mode === 'join') {
        setSas(await api.syncBeginJoin(code, label));
      } else {
        await api.syncRecover(server, workspaceId.trim(), phrase, label, local);
        await refreshSync();
        await actions.refresh();
        closeSyncDialog();
      }
    });

  const ready =
    label.trim() &&
    (mode === 'create'
      ? server.trim()
      : mode === 'join'
        ? code.trim()
        : server.trim() && workspaceId.trim() && phrase.trim());

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (ready && !busy) void submit();
      }}
    >
      <div className="segmented wizard-tabs" role="tablist" aria-label="Setup type">
        {(
          [
            ['create', 'Create workspace'],
            ['join', 'Join with a code'],
            ['recover', 'Recover'],
          ] as const
        ).map(([m, text]) => (
          <button
            key={m}
            type="button"
            role="tab"
            aria-selected={mode === m}
            className={mode === m ? 'is-on' : ''}
            onClick={() => {
              setMode(m);
              setError(null);
            }}
          >
            {text}
          </button>
        ))}
      </div>

      {mode === 'create' && (
        <p className="dialog-body">
          Start syncing the pages on this device. Everything is encrypted here before it is sent, so
          the server only stores data it cannot read.
        </p>
      )}
      {mode === 'join' && (
        <p className="dialog-body">
          On a device that already syncs, open Settings, then Sync, and choose Add device. Enter the
          code it shows.
        </p>
      )}
      {mode === 'recover' && (
        <p className="dialog-body">
          Lost every device? Enter your recovery key to get your notes back on this one.
        </p>
      )}

      {mode !== 'join' && (
        <Field label="Server" hint="Your own server, or one you were given.">
          <input
            className="input"
            value={server}
            onChange={(e) => setServer(e.target.value)}
            placeholder="https://sync.example.org"
            autoFocus
            spellCheck={false}
          />
        </Field>
      )}
      {mode === 'join' && (
        <Field label="Invite code">
          <input
            className="input mono"
            value={code}
            onChange={(e) => setCode(e.target.value)}
            placeholder="NCX1-…"
            autoFocus
            spellCheck={false}
          />
        </Field>
      )}
      {mode === 'recover' && (
        <>
          <Field label="Workspace ID">
            <input
              className="input mono"
              value={workspaceId}
              onChange={(e) => setWorkspaceId(e.target.value)}
              spellCheck={false}
            />
          </Field>
          <Field label="Recovery key">
            <input
              className="input mono"
              value={phrase}
              onChange={(e) => setPhrase(e.target.value)}
              placeholder="XXXX-XXXX-XXXX-…"
              spellCheck={false}
            />
          </Field>
        </>
      )}
      {mode === 'create' && (
        <Field label="Registration token" hint="Only if the server asks for one.">
          <input
            className="input"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            spellCheck={false}
          />
        </Field>
      )}
      <Field label="Name of this device">
        <input className="input" value={label} onChange={(e) => setLabel(e.target.value)} />
      </Field>
      {mode !== 'create' && <LocalChoice value={local} onChange={setLocal} />}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={closeSyncDialog}>
          Cancel
        </button>
        <button type="submit" className="btn btn-primary" disabled={!ready || busy}>
          {busy
            ? 'Working…'
            : mode === 'create'
              ? 'Create'
              : mode === 'join'
                ? 'Continue'
                : 'Recover'}
        </button>
      </div>
    </form>
  );
}

export function SyncWizard() {
  const open = useSync((s) => s.dialog === 'wizard');
  if (!open) return null;
  return (
    <Dialog title="Set up sync" onClose={closeSyncDialog}>
      <WizardBody />
    </Dialog>
  );
}

function AddDeviceBody() {
  const [offer, setOffer] = useState<InviteOffer | null>(null);
  const [request, setRequest] = useState<JoinRequest | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    api
      .syncCreateInvite()
      .then(setOffer)
      .catch((e) => setError(message(e)));
  }, []);

  useEffect(() => {
    if (!offer || request) return;
    let stop = false;
    const t = setInterval(() => {
      api
        .syncPendingJoin(offer.inviteId)
        .then((r) => !stop && r && setRequest(r))
        .catch((e) => !stop && setError(message(e)));
    }, 2000);
    return () => {
      stop = true;
      clearInterval(t);
    };
  }, [offer, request]);

  const approve = async () => {
    if (!offer) return;
    setBusy(true);
    try {
      await api.syncApproveJoin(offer.inviteId);
      await refreshSync();
      closeSyncDialog();
      actions.notify(`Added “${request?.label ?? 'device'}”.`, [
        'It will download your pages the next time it syncs.',
      ]);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      {!request && (
        <>
          <p className="dialog-body">
            On the new device, choose Set up sync, then Join with a code, and enter this code. It
            works once and expires in 15 minutes.
          </p>
          <textarea
            className="input mono invite-code"
            readOnly
            rows={4}
            value={offer?.code ?? 'Creating invitation…'}
            aria-label="Invite code"
            onFocus={(e) => e.currentTarget.select()}
          />
          <div className="kit-actions">
            <button
              className="btn"
              disabled={!offer}
              onClick={() => {
                void navigator.clipboard
                  ?.writeText(offer?.code ?? '')
                  .then(() => setCopied(true))
                  .catch(() => {});
              }}
            >
              {copied ? 'Copied' : 'Copy code'}
            </button>
          </div>
          {offer && (
            <p className="dialog-body" role="status">
              Waiting for the new device…
            </p>
          )}
        </>
      )}
      {request && (
        <>
          <p className="dialog-body">
            <strong>{request.label}</strong> wants to join your workspace. Approve it only if this
            code is shown on that device:
          </p>
          <div className="sas" aria-label="Verification code">
            {request.sas}
          </div>
          {!request.verified && (
            <p className="form-error" role="alert">
              This request did not use your invite code, so it did not come from the device you are
              setting up. Do not approve it.
            </p>
          )}
        </>
      )}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        <button className="btn" onClick={closeSyncDialog}>
          {request ? 'Decline' : 'Close'}
        </button>
        {request && (
          <button
            className="btn btn-primary"
            disabled={busy || !request.verified}
            onClick={() => void approve()}
          >
            Approve
          </button>
        )}
      </div>
    </>
  );
}

export function AddDeviceDialog() {
  const open = useSync((s) => s.dialog === 'addDevice');
  if (!open) return null;
  return (
    <Dialog title="Add a device" onClose={closeSyncDialog}>
      <AddDeviceBody />
    </Dialog>
  );
}
