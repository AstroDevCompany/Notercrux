import { actions, useApp } from '../state/app';
import { updateSettings, useSettings, type Settings } from '../state/settings';
import { Dialog } from './Dialog';

function Segmented<K extends keyof Settings>({
  label,
  description,
  field,
  options,
}: {
  label: string;
  description: string;
  field: K;
  options: { value: Settings[K]; label: string }[];
}) {
  const settings = useSettings();
  return (
    <div className="setting" role="radiogroup" aria-label={label}>
      <div>
        <div className="setting-label">{label}</div>
        <div className="setting-desc">{description}</div>
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

export function SettingsDialog() {
  const open = useApp((s) => s.settingsOpen);
  if (!open) return null;
  return (
    <Dialog title="Settings" wide onClose={() => actions.setSettings(false)}>
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
      <h3 className="dialog-sub">Keyboard</h3>
      <dl className="shortcuts">
        <dt>Command palette</dt>
        <dd>
          <kbd>⌘/Ctrl</kbd> <kbd>K</kbd>
        </dd>
        <dt>New page</dt>
        <dd>
          <kbd>⌘/Ctrl</kbd> <kbd>N</kbd>
        </dd>
        <dt>Toggle sidebar</dt>
        <dd>
          <kbd>⌘/Ctrl</kbd> <kbd>\</kbd>
        </dd>
        <dt>Settings</dt>
        <dd>
          <kbd>⌘/Ctrl</kbd> <kbd>,</kbd>
        </dd>
        <dt>Rename page (in sidebar)</dt>
        <dd>
          <kbd>F2</kbd>
        </dd>
        <dt>Archive page (in sidebar)</dt>
        <dd>
          <kbd>Delete</kbd>
        </dd>
      </dl>
      <div className="dialog-actions">
        <button className="btn btn-primary" onClick={() => actions.setSettings(false)}>
          Done
        </button>
      </div>
    </Dialog>
  );
}
