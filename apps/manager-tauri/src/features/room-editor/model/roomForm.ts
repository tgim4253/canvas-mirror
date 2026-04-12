import type {
  AvailableIccProfileDto,
  CreateRoomInput,
  DetectionMode,
  OutputResolution,
  StoredIccProfile,
  UpdateRoomInput,
} from '../../../shared/type';
import type { RoomCardView } from '../../../entities/room/model';

export const MODE_OPTIONS = [
  { value: 'watch', labelKey: 'roomForm.mode.watch' },
  { value: 'interval', labelKey: 'roomForm.mode.interval' },
];

export type ResolutionPreset = 'source' | 'hd' | 'fhd' | 'qhd' | 'uhd' | 'custom';
export type IccProfileSource = 'system' | 'file';

export const RESOLUTION_PRESETS = {
  hd: { max_width: 1280, max_height: 720, labelKey: 'roomForm.resolution.hd' },
  fhd: { max_width: 1920, max_height: 1080, labelKey: 'roomForm.resolution.fhd' },
  qhd: { max_width: 2560, max_height: 1440, labelKey: 'roomForm.resolution.qhd' },
  uhd: { max_width: 3840, max_height: 2160, labelKey: 'roomForm.resolution.uhd' },
} as const;

export const RESOLUTION_OPTIONS = [
  { value: 'source', labelKey: 'roomForm.resolution.source' },
  { value: 'hd', labelKey: RESOLUTION_PRESETS.hd.labelKey },
  { value: 'fhd', labelKey: RESOLUTION_PRESETS.fhd.labelKey },
  { value: 'qhd', labelKey: RESOLUTION_PRESETS.qhd.labelKey },
  { value: 'uhd', labelKey: RESOLUTION_PRESETS.uhd.labelKey },
  { value: 'custom', labelKey: 'roomForm.resolution.custom' },
];

export type RoomFormDraft = {
  name: string;
  detection_enabled: boolean;
  target_path: string;
  icc_profile_enabled: boolean;
  icc_profile_source: IccProfileSource;
  icc_profile_system_id: string | null;
  icc_profile_name: string;
  icc_profile_bytes: string | null;
  mode: DetectionMode;
  interval_ms: string;
  debounce_ms: string;
  stabilize_ms: string;
  resolution_preset: ResolutionPreset;
  max_width: string;
  max_height: string;
};

export type RoomFormFieldErrors = Partial<
  Record<
    | 'name'
    | 'target_path'
    | 'icc_profile'
    | 'interval_ms'
    | 'debounce_ms'
    | 'stabilize_ms'
    | 'max_width'
    | 'max_height',
    string
  >
>;

type NumericField = 'interval' | 'debounce' | 'stabilize' | 'maxWidth' | 'maxHeight';

const NUMERIC_FIELD_ERROR_KEYS: Record<NumericField, string> = {
  interval: 'roomForm.error.intervalMin',
  debounce: 'roomForm.error.debounceMin',
  stabilize: 'roomForm.error.stabilizeMin',
  maxWidth: 'roomForm.error.maxWidthMin',
  maxHeight: 'roomForm.error.maxHeightMin',
};

const NUMERIC_FIELD_INVALID_KEYS: Record<NumericField, string> = {
  interval: 'roomForm.error.intervalInvalid',
  debounce: 'roomForm.error.debounceInvalid',
  stabilize: 'roomForm.error.stabilizeInvalid',
  maxWidth: 'roomForm.error.maxWidthInvalid',
  maxHeight: 'roomForm.error.maxHeightInvalid',
};

export function createEmptyDraft(
  availableIccProfiles: AvailableIccProfileDto[] = [],
): RoomFormDraft {
  const defaultSystemProfile = defaultAvailableIccProfile(availableIccProfiles);

  return {
    name: '',
    detection_enabled: true,
    target_path: '',
    icc_profile_enabled: defaultSystemProfile !== null,
    icc_profile_source: defaultSystemProfile ? 'system' : 'file',
    icc_profile_system_id: defaultSystemProfile?.id ?? null,
    icc_profile_name: '',
    icc_profile_bytes: null,
    mode: 'watch',
    interval_ms: '2000',
    debounce_ms: '0',
    stabilize_ms: '0',
    resolution_preset: 'source',
    max_width: '1440',
    max_height: '1440',
  };
}

function resolveResolutionPreset(
  resolution: OutputResolution,
): Pick<RoomFormDraft, 'resolution_preset' | 'max_width' | 'max_height'> {
  if (resolution.kind === 'source') {
    return {
      resolution_preset: 'source',
      max_width: '1440',
      max_height: '1440',
    };
  }

  const matchedPreset = (
    Object.entries(RESOLUTION_PRESETS) as Array<
      [
        Exclude<ResolutionPreset, 'source' | 'custom'>,
        (typeof RESOLUTION_PRESETS)[keyof typeof RESOLUTION_PRESETS],
      ]
    >
  ).find(
    ([, preset]) =>
      preset.max_width === resolution.max_width && preset.max_height === resolution.max_height,
  );

  if (matchedPreset) {
    return {
      resolution_preset: matchedPreset[0],
      max_width: String(matchedPreset[1].max_width),
      max_height: String(matchedPreset[1].max_height),
    };
  }

  return {
    resolution_preset: 'custom',
    max_width: String(resolution.max_width),
    max_height: String(resolution.max_height),
  };
}

export function createDraftFromRoom(
  room: RoomCardView,
  configuredProfile: StoredIccProfile | null,
  availableIccProfiles: AvailableIccProfileDto[] = [],
): RoomFormDraft {
  const resolvedResolution = resolveResolutionPreset(room.room.room.resolution);
  const matchingSystemProfile =
    configuredProfile ? findMatchingAvailableIccProfile(configuredProfile, availableIccProfiles) : null;
  const defaultSystemProfile = defaultAvailableIccProfile(availableIccProfiles);
  const useFileSource = Boolean(configuredProfile && !matchingSystemProfile);

  return {
    name: room.room.room.name,
    detection_enabled: room.room.room.detection_enabled,
    target_path: room.target_path,
    icc_profile_enabled: room.room.room.icc_profile_enabled,
    icc_profile_source: useFileSource ? 'file' : defaultSystemProfile ? 'system' : 'file',
    icc_profile_system_id:
      matchingSystemProfile?.id ?? defaultSystemProfile?.id ?? null,
    icc_profile_name: useFileSource ? configuredProfile?.name ?? '' : '',
    icc_profile_bytes: useFileSource ? configuredProfile?.bytes ?? null : null,
    mode: room.room.room.mode,
    interval_ms: String(room.room.room.interval_ms),
    debounce_ms: String(room.room.room.debounce_ms),
    stabilize_ms: String(room.room.room.stabilize_ms),
    resolution_preset: resolvedResolution.resolution_preset,
    max_width: resolvedResolution.max_width,
    max_height: resolvedResolution.max_height,
  };
}

export function syncDraftWithAvailableIccProfiles(
  draft: RoomFormDraft,
  availableIccProfiles: AvailableIccProfileDto[],
): RoomFormDraft {
  if (draft.icc_profile_source === 'file') {
    return draft;
  }

  const defaultSystemProfile = defaultAvailableIccProfile(availableIccProfiles);
  if (!defaultSystemProfile) {
    return {
      ...draft,
      icc_profile_source: 'file',
      icc_profile_system_id: null,
    };
  }

  if (
    draft.icc_profile_system_id &&
    availableIccProfiles.some(profile => profile.id === draft.icc_profile_system_id)
  ) {
    return draft;
  }

  return {
    ...draft,
    icc_profile_system_id: defaultSystemProfile.id,
  };
}

export function deriveRoomNameFromTargetPath(targetPath: string): string {
  const normalizedPath = targetPath.trim().replace(/[\\/]+$/g, '');

  if (!normalizedPath) {
    return '';
  }

  const segments = normalizedPath.split(/[/\\]/);
  const fileName = segments[segments.length - 1] ?? '';
  if (!fileName) {
    return '';
  }

  const extensionIndex = fileName.lastIndexOf('.');
  if (extensionIndex <= 0) {
    return fileName;
  }

  return fileName.slice(0, extensionIndex);
}

function defaultAvailableIccProfile(
  availableIccProfiles: AvailableIccProfileDto[],
): AvailableIccProfileDto | null {
  return availableIccProfiles.find(profile => profile.is_primary) ?? availableIccProfiles[0] ?? null;
}

function findMatchingAvailableIccProfile(
  configuredProfile: StoredIccProfile,
  availableIccProfiles: AvailableIccProfileDto[],
): AvailableIccProfileDto | null {
  return (
    availableIccProfiles.find(profile => profile.profile.bytes === configuredProfile.bytes) ??
    availableIccProfiles.find(profile => profile.profile.name === configuredProfile.name) ??
    null
  );
}

function resolveSelectedIccProfile(
  draft: RoomFormDraft,
  availableIccProfiles: AvailableIccProfileDto[],
): StoredIccProfile | null {
  if (draft.icc_profile_source === 'file') {
    if (!draft.icc_profile_name.trim() || !draft.icc_profile_bytes) {
      return null;
    }

    return {
      name: draft.icc_profile_name.trim(),
      bytes: draft.icc_profile_bytes,
    };
  }

  const selectedProfile = availableIccProfiles.find(
    profile => profile.id === draft.icc_profile_system_id,
  );
  return selectedProfile?.profile ?? null;
}

function readPositiveInteger(field: NumericField, value: string, minimum: number) {
  const parsed = Number.parseInt(value, 10);

  if (!Number.isFinite(parsed) || Number.isNaN(parsed) || parsed < minimum) {
    throw new Error(NUMERIC_FIELD_ERROR_KEYS[field]);
  }

  return parsed;
}

function buildSharedRoomInput(draft: RoomFormDraft) {
  const name = draft.name.trim();
  const targetPath = draft.target_path.trim();

  if (!name) {
    throw new Error('roomForm.error.roomNameRequired');
  }
  if (!targetPath) {
    throw new Error('roomForm.error.targetPathRequired');
  }

  const resolution =
    draft.resolution_preset === 'source'
      ? { kind: 'source' as const }
      : draft.resolution_preset === 'custom'
        ? {
            kind: 'contain' as const,
            max_width: readPositiveInteger('maxWidth', draft.max_width, 1),
            max_height: readPositiveInteger('maxHeight', draft.max_height, 1),
          }
        : {
            kind: 'contain' as const,
            max_width: RESOLUTION_PRESETS[draft.resolution_preset].max_width,
            max_height: RESOLUTION_PRESETS[draft.resolution_preset].max_height,
          };

  return {
    name,
    detection_enabled: draft.detection_enabled,
    target_path: targetPath,
    mode: draft.mode,
    interval_ms: readPositiveInteger('interval', draft.interval_ms, 1),
    debounce_ms: readPositiveInteger('debounce', draft.debounce_ms, 0),
    stabilize_ms: readPositiveInteger('stabilize', draft.stabilize_ms, 0),
    resolution,
    icc_profile_enabled: draft.icc_profile_enabled,
  };
}

export function buildCreateRoomInput(
  draft: RoomFormDraft,
  availableIccProfiles: AvailableIccProfileDto[],
): CreateRoomInput {
  return {
    ...buildSharedRoomInput(draft),
    icc_profile: buildIccProfileInput(draft, availableIccProfiles),
  };
}

export function buildUpdateRoomInput(
  draft: RoomFormDraft,
  availableIccProfiles: AvailableIccProfileDto[],
): UpdateRoomInput {
  return {
    ...buildSharedRoomInput(draft),
    icc_profile: buildIccProfileInput(draft, availableIccProfiles),
  };
}

function buildIccProfileInput(
  draft: RoomFormDraft,
  availableIccProfiles: AvailableIccProfileDto[],
): StoredIccProfile | null {
  const iccProfile = resolveSelectedIccProfile(draft, availableIccProfiles);

  if (draft.icc_profile_enabled && !iccProfile) {
    throw new Error('roomForm.error.iccProfileRequired');
  }

  return iccProfile;
}

export function validateRoomDraft(
  draft: RoomFormDraft,
  availableIccProfiles: AvailableIccProfileDto[],
): RoomFormFieldErrors {
  const errors: RoomFormFieldErrors = {};

  if (!draft.name.trim()) {
    errors.name = 'roomForm.error.roomNameRequired';
  }
  if (!draft.target_path.trim()) {
    errors.target_path = 'roomForm.error.targetPathRequired';
  }
  if (draft.icc_profile_enabled && !resolveSelectedIccProfile(draft, availableIccProfiles)) {
    errors.icc_profile = 'roomForm.error.iccProfileRequired';
  }

  if (draft.mode === 'interval') {
    try {
      readPositiveInteger('interval', draft.interval_ms, 1);
    } catch (error) {
      errors.interval_ms =
        error instanceof Error ? error.message : NUMERIC_FIELD_INVALID_KEYS.interval;
    }
  }

  if (draft.mode === 'watch') {
    try {
      readPositiveInteger('debounce', draft.debounce_ms, 0);
    } catch (error) {
      errors.debounce_ms =
        error instanceof Error ? error.message : NUMERIC_FIELD_INVALID_KEYS.debounce;
    }

    try {
      readPositiveInteger('stabilize', draft.stabilize_ms, 0);
    } catch (error) {
      errors.stabilize_ms =
        error instanceof Error ? error.message : NUMERIC_FIELD_INVALID_KEYS.stabilize;
    }
  }

  if (draft.resolution_preset === 'custom') {
    try {
      readPositiveInteger('maxWidth', draft.max_width, 1);
    } catch (error) {
      errors.max_width =
        error instanceof Error ? error.message : NUMERIC_FIELD_INVALID_KEYS.maxWidth;
    }

    try {
      readPositiveInteger('maxHeight', draft.max_height, 1);
    } catch (error) {
      errors.max_height =
        error instanceof Error ? error.message : NUMERIC_FIELD_INVALID_KEYS.maxHeight;
    }
  }

  return errors;
}

export function resolveRoomFormFieldErrors(message: string): RoomFormFieldErrors {
  if (
    message === 'roomForm.error.roomNameRequired' ||
    message === 'Room name is required.'
  ) {
    return { name: message };
  }

  if (
    message === 'roomForm.error.targetPathRequired' ||
    message === 'Target path is required.'
  ) {
    return { target_path: message };
  }

  if (
    message === 'roomForm.error.iccProfileRequired' ||
    message === 'ICC profile is required.' ||
    message.includes('has ICC enabled but no ICC profile is configured')
  ) {
    return { icc_profile: message };
  }

  if (
    message === 'roomForm.error.intervalMin' ||
    message === 'roomForm.error.intervalInvalid' ||
    message.startsWith('Interval')
  ) {
    return { interval_ms: message };
  }

  if (
    message === 'roomForm.error.debounceMin' ||
    message === 'roomForm.error.debounceInvalid' ||
    message.startsWith('Debounce')
  ) {
    return { debounce_ms: message };
  }

  if (
    message === 'roomForm.error.stabilizeMin' ||
    message === 'roomForm.error.stabilizeInvalid' ||
    message.startsWith('Stabilize')
  ) {
    return { stabilize_ms: message };
  }

  if (
    message === 'roomForm.error.maxWidthMin' ||
    message === 'roomForm.error.maxWidthInvalid' ||
    message.startsWith('Max width')
  ) {
    return { max_width: message };
  }

  if (
    message === 'roomForm.error.maxHeightMin' ||
    message === 'roomForm.error.maxHeightInvalid' ||
    message.startsWith('Max height')
  ) {
    return { max_height: message };
  }

  return {};
}
