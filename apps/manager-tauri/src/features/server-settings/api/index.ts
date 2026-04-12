import { invokeTauri } from '../../../shared/lib/tauri';
import type { ServerSettingsDto, UpdateServerSettingsInput } from '../../../shared/type';

export function getServerSettings() {
  return invokeTauri<ServerSettingsDto>('get_server_settings');
}

export function updateServerSettings(input: UpdateServerSettingsInput) {
  return invokeTauri<ServerSettingsDto>('update_server_settings', { input });
}
