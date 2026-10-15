import { useEffect } from 'react';
import { isTauri } from '../api';
import { useSettings, type Settings } from '../state/settings';

const isMac = /Mac/i.test(navigator.platform);
const isWindows = /Win/i.test(navigator.platform);

const mq = (q: string) => window.matchMedia(q);

export function resolveTheme(s: Settings): 'light' | 'dark' {
  if (s.theme !== 'system') return s.theme;
  return mq('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}
export function resolveReducedTransparency(s: Settings): boolean {
  if (s.transparency !== 'system') return s.transparency === 'reduced';
  return mq('(prefers-reduced-transparency: reduce)').matches;
}
export function resolveReducedMotion(s: Settings): boolean {
  if (s.motion !== 'system') return s.motion === 'reduced';
  return mq('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Applies appearance settings to the document and to the native window material.
 * Reduced transparency removes both the CSS glass and the native vibrancy/mica layer, leaving
 * a fully opaque (but still designed) interface.
 */
export function useMaterial() {
  const settings = useSettings();

  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      const theme = resolveTheme(settings);
      const reduced = resolveReducedTransparency(settings);
      root.dataset.theme = theme;
      root.dataset.transparency = reduced ? 'reduced' : 'glass';
      root.dataset.motion = resolveReducedMotion(settings) ? 'reduced' : 'full';
      root.dataset.native = isTauri ? 'true' : 'false';
      root.dataset.platform = isMac ? 'mac' : isWindows ? 'windows' : 'other';

      if (isTauri) {
        void (async () => {
          try {
            const { getCurrentWindow } = await import('@tauri-apps/api/window');
            const win = getCurrentWindow();
            await win.setTheme(settings.theme === 'system' ? null : settings.theme);
            if (reduced) await win.clearEffects();
            else if (isMac)
              await win.setEffects({
                effects: ['sidebar' as never],
                state: 'followsWindowActiveState' as never,
              });
            else if (isWindows) await win.setEffects({ effects: ['mica' as never] });
          } catch {
            /* unsupported platform or permission: the CSS fallback still applies */
          }
        })();
      }
    };
    apply();
    const queries = [
      '(prefers-color-scheme: dark)',
      '(prefers-reduced-transparency: reduce)',
      '(prefers-reduced-motion: reduce)',
    ].map(mq);
    queries.forEach((q) => q.addEventListener('change', apply));
    return () => queries.forEach((q) => q.removeEventListener('change', apply));
  }, [settings]);
}
