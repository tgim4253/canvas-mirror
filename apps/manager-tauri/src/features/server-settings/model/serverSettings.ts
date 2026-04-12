import type { ServerSettingsDto, UpdateServerSettingsInput } from '../../../shared/type';

export type ServerSettingsDraft = {
  bind_host: string;
  bind_port: string;
  public_url: string;
  stale_timeout_ms: string;
};

export type ServerSettingsFieldErrors = Partial<
  Record<'bind_host' | 'bind_port' | 'public_url' | 'stale_timeout_ms', string>
>;

export const LOOPBACK_BIND_HOST = '127.0.0.1';
export const WILDCARD_BIND_HOST = '0.0.0.0';
export const BIND_HOST_PRESETS = ['loopback', 'wildcard'] as const;

export type BindHostPreset = (typeof BIND_HOST_PRESETS)[number];

export function createEmptyServerSettingsDraft(): ServerSettingsDraft {
  return {
    bind_host: WILDCARD_BIND_HOST,
    bind_port: '8787',
    public_url: '',
    stale_timeout_ms: '30000',
  };
}

export function createDraftFromServerSettings(
  settings: ServerSettingsDto,
): ServerSettingsDraft {
  return {
    bind_host: settings.bind_host,
    bind_port: String(settings.bind_port),
    public_url: settings.public_url ?? '',
    stale_timeout_ms: String(settings.stale_timeout_ms),
  };
}

export function buildUpdateServerSettingsInput(
  draft: ServerSettingsDraft,
): UpdateServerSettingsInput {
  const bindHost = draft.bind_host.trim();
  if (!bindHost) {
    throw new Error('serverSettings.error.bindHostRequired');
  }

  const bindPort = Number.parseInt(draft.bind_port, 10);
  if (!Number.isInteger(bindPort) || bindPort < 1 || bindPort > 65_535) {
    throw new Error('serverSettings.error.portRange');
  }

  const staleTimeoutMs = Number.parseInt(draft.stale_timeout_ms, 10);
  if (!Number.isInteger(staleTimeoutMs) || staleTimeoutMs < 0) {
    throw new Error('serverSettings.error.staleTimeoutRange');
  }

  const publicUrl = draft.public_url.trim();

  return {
    bind_host: bindHost,
    bind_port: bindPort,
    public_url: publicUrl || null,
    stale_timeout_ms: staleTimeoutMs,
  };
}

export function validateServerSettingsDraft(
  draft: ServerSettingsDraft,
): ServerSettingsFieldErrors {
  const errors: ServerSettingsFieldErrors = {};

  if (!draft.bind_host.trim()) {
    errors.bind_host = 'serverSettings.error.bindHostRequired';
  }

  const bindPort = Number.parseInt(draft.bind_port, 10);
  if (!Number.isInteger(bindPort) || bindPort < 1 || bindPort > 65_535) {
    errors.bind_port = 'serverSettings.error.portRange';
  }

  const publicUrl = draft.public_url.trim();
  if (publicUrl) {
    try {
      const parsed = new URL(publicUrl);
      if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
        errors.public_url = 'serverSettings.error.publicUrlHttpScheme';
      }
    } catch {
      errors.public_url = 'serverSettings.error.publicUrlValid';
    }
  }

  const staleTimeoutMs = Number.parseInt(draft.stale_timeout_ms, 10);
  if (!Number.isInteger(staleTimeoutMs) || staleTimeoutMs < 0) {
    errors.stale_timeout_ms = 'serverSettings.error.staleTimeoutRange';
  }

  return errors;
}

export function resolveServerSettingsFieldErrors(
  message: string,
): ServerSettingsFieldErrors {
  if (
    message === 'serverSettings.error.bindHostRequired' ||
    message === 'Bind host is required.' ||
    message.startsWith('invalid bind host')
  ) {
    return { bind_host: message };
  }

  if (
    message === 'serverSettings.error.portRange' ||
    message === 'Port must be between 1 and 65535.'
  ) {
    return { bind_port: message };
  }

  if (
    message === 'serverSettings.error.publicUrlValid' ||
    message === 'serverSettings.error.publicUrlHttpScheme' ||
    message === 'Public URL must be a valid URL.' ||
    message === 'Public URL must use http or https.' ||
    message.startsWith('invalid public URL') ||
    message.startsWith('unsupported public URL scheme')
  ) {
    return { public_url: message };
  }

  if (
    message === 'serverSettings.error.staleTimeoutRange' ||
    message === 'Stale timeout must be 0 or higher.'
  ) {
    return { stale_timeout_ms: message };
  }

  return {};
}

const IPV6_LOOPBACK = '::1';
const EXPANDED_IPV6_LOOPBACK = '0:0:0:0:0:0:0:1';
const IPV6_UNSPECIFIED = '::';
const EXPANDED_IPV6_UNSPECIFIED = '0:0:0:0:0:0:0:0';

function isLoopbackBindHost(bindHost: string) {
  return (
    bindHost.startsWith('127.') ||
    bindHost === IPV6_LOOPBACK ||
    bindHost === EXPANDED_IPV6_LOOPBACK
  );
}

function isWildcardBindHost(bindHost: string) {
  return (
    bindHost === WILDCARD_BIND_HOST ||
    bindHost === IPV6_UNSPECIFIED ||
    bindHost === EXPANDED_IPV6_UNSPECIFIED
  );
}

export function bindHostPresetFromHost(bindHost: string): BindHostPreset {
  const normalizedBindHost = bindHost.trim();

  if (!normalizedBindHost || isLoopbackBindHost(normalizedBindHost)) {
    return 'loopback';
  }

  return 'wildcard';
}

export function bindHostForPreset(preset: BindHostPreset): string {
  return preset === 'loopback' ? LOOPBACK_BIND_HOST : WILDCARD_BIND_HOST;
}

export function describeCustomBindHost(bindHost: string) {
  const normalizedBindHost = bindHost.trim();
  if (
    !normalizedBindHost ||
    normalizedBindHost === LOOPBACK_BIND_HOST ||
    normalizedBindHost === WILDCARD_BIND_HOST
  ) {
    return null;
  }

  return normalizedBindHost;
}

export function describeServerExposureWarning(draft: ServerSettingsDraft) {
  const bindHost = draft.bind_host.trim();
  if (!bindHost || isLoopbackBindHost(bindHost)) {
    return null;
  }

  if (isWildcardBindHost(bindHost)) {
    if (!draft.public_url.trim()) {
      return 'serverSettings.warning.networkExposedAutoDerived';
    }

    return 'serverSettings.warning.networkExposedPublicUrl';
  }

  return 'serverSettings.warning.nonLoopback';
}
