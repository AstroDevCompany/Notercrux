import { useEffect, useState } from 'react';
import { actions, useApp } from '../state/app';
import {
  COMMANDS,
  conflictOf,
  eventToBinding,
  formatBinding,
  isValidBinding,
  resetBindings,
  setBinding,
  useBindings,
  type CommandId,
} from '../state/shortcuts';
import { DEFAULTS, updateSettings, useSettings, type Settings } from '../state/settings';
import logo from '../assets/mintflow-logo.webp';
import { AppLogo } from './AppLogo';
import { Dialog } from './Dialog';

function Segmented<K extends keyof Settings>({
  label,
  description,
  field,
  options,
}: {
  label: string;
  description?: string;
  field: K;
  options: { value: Settings[K]; label: string }[];
}) {
  const settings = useSettings();
  return (
    <div className="setting" role="radiogroup" aria-label={label}>
      <div>
        <div className="setting-label">{label}</div>
        {description && <div className="setting-desc">{description}</div>}
      </div>
      <div className="segmented">
        {options.map((o) => (
          <button
            key={o.value}
            role="radio"
            aria-checked={settings[field] === o.value}
            className={settings[field] === o.value ? 'is-on' : ''}
            onClick={() => updateSettings({ [field]: o.value } as Partial<Settings>)}
          >
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

const ACCENTS: { value: Settings['accent']; color: string }[] = [
  { value: 'mint', color: '#0f8a72' },
  { value: 'indigo', color: '#5865f2' },
  { value: 'blue', color: '#2f7cf6' },
  { value: 'green', color: '#2e9e6b' },
  { value: 'pink', color: '#d6478a' },
  { value: 'orange', color: '#e0742a' },
  { value: 'graphite', color: '#5d6470' },
];

function AccentPicker() {
  const { accent } = useSettings();
  return (
    <div className="setting" role="radiogroup" aria-label="Accent color">
      <div>
        <div className="setting-label">Accent color</div>
        <div className="setting-desc">Selection, links and primary buttons.</div>
      </div>
      <div className="swatches">
        {ACCENTS.map((a) => (
          <button
            key={a.value}
            role="radio"
            aria-checked={accent === a.value}
            aria-label={a.value}
            title={a.value}
            className={`swatch${accent === a.value ? ' is-on' : ''}`}
            style={{ background: a.color }}
            onClick={() => updateSettings({ accent: a.value })}
          />
        ))}
      </div>
    </div>
  );
}

function ShortcutRow({ id, label }: { id: CommandId; label: string }) {
  const bindings = useBindings();
  const [recording, setRecording] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const def = COMMANDS.find((c) => c.id === id)!;

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') return setRecording(false);
      const b = eventToBinding(e);
      if (!b) return; // still holding modifiers
      if (!isValidBinding(b)) {
        setMessage('Include ⌘/Ctrl or Alt so typing never triggers it.');
        return;
      }
      const clash = conflictOf(b, id);
      if (clash) {
        setMessage(`Already used by “${clash.label}”.`);
        return;
      }
      setBinding(id, b);
      setMessage(null);
      setRecording(false);
    };
    // Capture phase so the dialog's own Escape/Tab handling does not swallow the keys.
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [recording, id]);

  return (
    <div className="shortcut-row">
      <span>{label}</span>
      <span className="shortcut-controls">
        {message && (
          <span className="shortcut-msg" role="alert">
            {message}
          </span>
        )}
        <button
          className={`shortcut-btn${recording ? ' is-recording' : ''}`}
          aria-label={`${label} shortcut: ${formatBinding(bindings[id])}. Click to change.`}
          onClick={() => {
            setMessage(null);
            setRecording(!recording);
          }}
          onBlur={() => setRecording(false)}
        >
          {recording ? 'Press keys…' : formatBinding(bindings[id])}
        </button>
        {bindings[id] !== def.default && (
          <button
            className="icon-btn"
            aria-label={`Reset ${label} shortcut`}
            title="Reset"
            onClick={() => setBinding(id, null)}
          >
            ↺
          </button>
        )}
      </span>
    </div>
  );
}

export function SettingsDialog() {
  const open = useApp((s) => s.settingsOpen);
  if (!open) return null;
  return (
    <Dialog title="Settings" wide onClose={() => actions.setSettings(false)}>
      <div className="settings-scroll">
        <h3 className="dialog-sub">Appearance</h3>
        <Segmented
          label="Theme"
          description="Light and dark are tuned separately."
          field="theme"
          options={[
            { value: 'system', label: 'System' },
            { value: 'light', label: 'Light' },
            { value: 'dark', label: 'Dark' },
          ]}
        />
        <AccentPicker />
        <Segmented
          label="Glass effects"
          description="Translucent surfaces and native window blur. Reduced uses solid surfaces."
          field="transparency"
          options={[
            { value: 'system', label: 'System' },
            { value: 'glass', label: 'Glass' },
            { value: 'reduced', label: 'Reduced' },
          ]}
        />
        <Segmented
          label="Motion"
          description="Animations and transitions."
          field="motion"
          options={[
            { value: 'system', label: 'System' },
            { value: 'full', label: 'Full' },
            { value: 'reduced', label: 'Reduced' },
          ]}
        />

        <h3 className="dialog-sub">Pages</h3>
        <Segmented
          label="Typeface"
          field="font"
          options={[
            { value: 'sans', label: 'Sans' },
            { value: 'serif', label: 'Serif' },
            { value: 'mono', label: 'Mono' },
          ]}
        />
        <Segmented
          label="Text size"
          field="textSize"
          options={[
            { value: 'small', label: 'Small' },
            { value: 'medium', label: 'Medium' },
            { value: 'large', label: 'Large' },
          ]}
        />
        <Segmented
          label="Page width"
          field="pageWidth"
          options={[
            { value: 'narrow', label: 'Narrow' },
            { value: 'default', label: 'Default' },
            { value: 'wide', label: 'Wide' },
            { value: 'full', label: 'Full' },
          ]}
        />

        <h3 className="dialog-sub">Keyboard shortcuts</h3>
        {COMMANDS.map((c) => (
          <ShortcutRow key={c.id} id={c.id} label={c.label} />
        ))}
        <p className="setting-desc" style={{ maxWidth: 'none', marginTop: 8 }}>
          Editor shortcuts (formatting, undo, moving blocks) and sidebar keys follow platform
          conventions and are not rebindable.
        </p>
      </div>
      <div className="about-app">
        <AppLogo size={40} />
        <div>
          <strong>Notercrux</strong>
          <span>Local-first notes and databases</span>
        </div>
      </div>
      <div className="about">
        <img
          src={logo}
          alt="MintFlow Technologies"
          className="about-logo"
          width={112}
          height={26}
        />
        <p>
          Notercrux is made by MintFlow Technologies.
          <br />
          Lead developer: <strong>Luca Montanari</strong>
        </p>
      </div>
      <div className="dialog-actions">
        <button
          className="btn"
          onClick={() => {
            updateSettings({ ...DEFAULTS });
            resetBindings();
          }}
        >
          Reset all
        </button>
        <button
          className="btn btn-primary"
          data-autofocus
          onClick={() => actions.setSettings(false)}
        >
          Done
        </button>
      </div>
    </Dialog>
  );
}
