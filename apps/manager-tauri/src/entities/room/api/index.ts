import { invokeTauri, listenTauri } from '../../../shared/lib/tauri';
import type {
  AvailableIccProfileDto,
  CreateRoomInput,
  ManagedRoomDto,
  RoomPreviewDto,
  RuntimeLogsChangedDto,
  ServerStatusDto,
  StoredIccProfile,
  UpdateRoomInput,
} from '../../../shared/type';
import {
  STUDIO_ROOM_PREVIEWS_CHANGED_EVENT as ROOM_PREVIEWS_CHANGED_EVENT,
  STUDIO_ROOMS_CHANGED_EVENT as ROOMS_CHANGED_EVENT,
  STUDIO_RUNTIME_LOGS_CHANGED_EVENT as RUNTIME_LOGS_CHANGED_EVENT,
} from '../../../shared/type';

export function listRooms() {
  return invokeTauri<ManagedRoomDto[]>('list_rooms');
}

export function createRoom(input: CreateRoomInput) {
  return invokeTauri<ManagedRoomDto>('create_room', { input });
}

export function updateRoom(roomId: string, input: UpdateRoomInput) {
  return invokeTauri<ManagedRoomDto>('update_room', { roomId, input });
}

export function loadIccProfile(path: string) {
  return invokeTauri<StoredIccProfile>('load_icc_profile', { path });
}

export function listAvailableIccProfiles() {
  return invokeTauri<AvailableIccProfileDto[]>('list_available_icc_profiles');
}

export function getRoomIccProfile(roomId: string) {
  return invokeTauri<StoredIccProfile | null>('get_room_icc_profile', { roomId });
}

export function deleteRoom(roomId: string) {
  return invokeTauri<ManagedRoomDto>('delete_room', { roomId });
}

export function setRoomRunning(roomId: string, running: boolean) {
  return invokeTauri<ManagedRoomDto>('set_room_running', { roomId, running });
}

export function getServerStatus() {
  return invokeTauri<ServerStatusDto>('get_server_status');
}

export function listenRoomsChanged(handler: (rooms: ManagedRoomDto[]) => void) {
  return listenTauri<ManagedRoomDto[]>(ROOMS_CHANGED_EVENT, handler);
}

export function listenRoomPreviewsChanged(handler: (previews: RoomPreviewDto[]) => void) {
  return listenTauri<RoomPreviewDto[]>(ROOM_PREVIEWS_CHANGED_EVENT, handler);
}

export function listenRuntimeLogsChanged(handler: (payload: RuntimeLogsChangedDto) => void) {
  return listenTauri<RuntimeLogsChangedDto>(RUNTIME_LOGS_CHANGED_EVENT, handler);
}
