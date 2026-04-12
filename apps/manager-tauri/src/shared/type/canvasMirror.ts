// Mirrors DTOs serialized by crates/canvas-mirror-model and crates/canvas-mirror-store.
export type IsoDateTimeString = string;
export type UrlString = string;

export const DETECTION_MODES = ['watch', 'interval'] as const;
export type DetectionMode = (typeof DETECTION_MODES)[number];

export type OutputResolution =
  | {
      kind: 'source';
    }
  | {
      kind: 'contain';
      max_width: number;
      max_height: number;
    };

export const ROOM_STATES = ['running', 'paused', 'error'] as const;
export type RoomState = (typeof ROOM_STATES)[number];

export const DEVICE_STATES = ['online', 'offline', 'stale', 'paused'] as const;
export type DeviceState = (typeof DEVICE_STATES)[number];

export const DEVICE_PLATFORMS = ['desktop', 'tablet', 'phone', 'unknown'] as const;
export type DevicePlatform = (typeof DEVICE_PLATFORMS)[number];

export const LOG_LEVELS = ['info', 'warn', 'error'] as const;
export type LogLevel = (typeof LOG_LEVELS)[number];

export type SnapshotMetaDto = {
  room_id: string;
  content_hash: string;
  mime_type: string;
  bytes_len: number;
  width: number | null;
  height: number | null;
  created_at: IsoDateTimeString;
};

export type RoomDeviceDto = {
  id: string;
  name: string;
  platform: DevicePlatform;
  screen_width: number | null;
  screen_height: number | null;
  state: DeviceState;
  last_seen_at: IsoDateTimeString | null;
};

export type RoomSummaryDto = {
  id: string;
  name: string;
  detection_enabled: boolean;
  mode: DetectionMode;
  interval_ms: number;
  debounce_ms: number;
  stabilize_ms: number;
  resolution: OutputResolution;
};

export type RoomDto = {
  room: RoomSummaryDto;
  state: RoomState;
  devices: RoomDeviceDto[];
  latest_snapshot: SnapshotMetaDto | null;
  last_error: string | null;
};

export type LogEntryDto = {
  at: IsoDateTimeString;
  level: LogLevel;
  scope: string;
  message: string;
};

export type ServerStatusDto = {
  generated_at: IsoDateTimeString;
  public_url: UrlString | null;
  rooms: RoomDto[];
  logs: LogEntryDto[];
};
