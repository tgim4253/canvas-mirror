import type {
  DetectionMode,
  IsoDateTimeString,
  LogEntryDto,
  OutputResolution,
  RoomDto,
  SnapshotMetaDto,
  UrlString,
} from './canvasMirror';

export const STUDIO_ROOMS_CHANGED_EVENT = 'studio://rooms-changed';
export const STUDIO_ROOM_PREVIEWS_CHANGED_EVENT = 'studio://room-previews-changed';
export const STUDIO_RUNTIME_LOGS_CHANGED_EVENT = 'studio://runtime-logs-changed';

export type ManagedRoomViewerLinkDto = {
  viewer_url: UrlString;
  qr_svg: string | null;
};

export type ManagedRoomDto = {
  room: RoomDto;
  target_path: string;
  viewer_links: ManagedRoomViewerLinkDto[];
  preview_data_url: UrlString | null;
};

export type RoomPreviewDto = {
  room_id: string;
  preview_data_url: UrlString | null;
  latest_snapshot: SnapshotMetaDto | null;
};

export type RuntimeLogsChangedDto = {
  generated_at: IsoDateTimeString;
  logs: LogEntryDto[];
  replace: boolean;
};

export type CreateRoomInput = {
  name: string;
  detection_enabled: boolean;
  target_path: string;
  mode: DetectionMode;
  interval_ms: number;
  debounce_ms: number;
  stabilize_ms: number;
  resolution: OutputResolution;
};

export type UpdateRoomInput = {
  name: string;
  detection_enabled: boolean;
  target_path: string;
  mode: DetectionMode;
  interval_ms: number;
  debounce_ms: number;
  stabilize_ms: number;
  resolution: OutputResolution;
};

export type ServerSettingsDto = {
  bind_host: string;
  bind_port: number;
  public_url: UrlString | null;
  stale_timeout_ms: number;
};

export type UpdateServerSettingsInput = {
  bind_host: string;
  bind_port: number;
  public_url: UrlString | null;
  stale_timeout_ms: number;
};
