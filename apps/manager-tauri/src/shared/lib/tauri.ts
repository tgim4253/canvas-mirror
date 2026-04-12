import { detectPreferredLocale } from '../i18n';
import { translateForLocale } from '../i18n/resources';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function isTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export async function invokeTauri<T>(command: string, args?: Record<string, unknown>) {
  if (!isTauriRuntime()) {
    throw new Error('tauri.error.runtimeUnavailable');
  }

  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

export async function listenTauri<T>(
  eventName: string,
  handler: (payload: T) => void,
) {
  if (!isTauriRuntime()) {
    throw new Error('tauri.error.runtimeUnavailable');
  }

  const { listen } = await import('@tauri-apps/api/event');
  return listen<T>(eventName, event => {
    handler(event.payload);
  });
}

export async function pickFilePath() {
  if (!isTauriRuntime()) {
    throw new Error('tauri.error.filePickerOnlyInTauri');
  }

  const { open } = await import('@tauri-apps/plugin-dialog');
  const selected = await open({
    directory: false,
    multiple: false,
    title: translateForLocale(
      detectPreferredLocale(),
      'tauri.dialog.selectTargetFile',
    ),
  });

  if (selected === null) {
    return null;
  }

  return Array.isArray(selected) ? (selected[0] ?? null) : selected;
}
