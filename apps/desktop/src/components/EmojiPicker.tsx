import { useState, type RefObject } from 'react';
import { CALLOUT_ICONS, CALLOUT_TONES } from '../editor/languages';
import { Popover, type Anchor } from './database/Popover';

/** Icon (and optionally colour) picker for callouts. Any emoji can be pasted into the text box. */
export function IconPicker({
  anchor,
  icon,
  tone,
  onIcon,
  onTone,
  onClose,
}: {
  anchor: RefObject<HTMLElement | null> | Anchor;
  icon: string;
  tone?: string;
  onIcon: (icon: string) => void;
  onTone?: (tone: string) => void;
  onClose: () => void;
}) {
  const [custom, setCustom] = useState('');
  return (
    <Popover anchor={anchor} label="Choose callout icon" width={272} onClose={onClose}>
      <div className="icon-picker">
        <div className="icon-grid" role="listbox" aria-label="Icons">
          {CALLOUT_ICONS.map((e) => (
            <button
              key={e}
              role="option"
              aria-selected={e === icon}
              className={`icon-cell${e === icon ? ' is-on' : ''}`}
              onClick={() => {
                onIcon(e);
                if (!onTone) onClose();
              }}
            >
              {e}
            </button>
          ))}
        </div>
        <div className="icon-custom">
          <input
            className="pop-input"
            aria-label="Any emoji"
            placeholder="Paste any emoji"
            value={custom}
            onChange={(e) => setCustom(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && custom.trim()) {
                onIcon(Array.from(custom.trim())[0]!);
                onClose();
              }
            }}
          />
        </div>
        {onTone && (
          <div className="tone-row" role="radiogroup" aria-label="Color">
            {CALLOUT_TONES.map((t) => (
              <button
                key={t.id}
                role="radio"
                aria-checked={(tone ?? 'default') === t.id}
                aria-label={t.label}
                title={t.label}
                className={`swatch${(tone ?? 'default') === t.id ? ' is-on' : ''}`}
                style={{ background: t.swatch }}
                onClick={() => onTone(t.id)}
              />
            ))}
          </div>
        )}
      </div>
    </Popover>
  );
}
