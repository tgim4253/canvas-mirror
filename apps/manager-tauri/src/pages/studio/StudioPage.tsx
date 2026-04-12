import { startTransition, type FormEvent, useEffect, useState } from 'react';
import {
  createRoom,
  deleteRoom,
  getServerStatus,
  listenRoomPreviewsChanged,
  listenRoomsChanged,
  listenRuntimeLogsChanged,
  listRooms,
  setRoomRunning,
  updateRoom,
} from '../../entities/room/api';
import { type RoomCardView, toRoomCardProps } from '../../entities/room/model';
import { RoomAccordionList, RoomCard } from '../../entities/room/ui';
import {
  buildCreateRoomInput,
  buildUpdateRoomInput,
  createDraftFromRoom,
  createEmptyDraft,
  deriveRoomNameFromTargetPath,
  resolveRoomFormFieldErrors,
  RoomEditorModal,
  type RoomFormFieldErrors,
  type RoomFormDraft,
  validateRoomDraft,
} from '../../features/room-editor';
import { RoomDeleteModal } from '../../features/room-delete';
import { RoomQrModal } from '../../features/room-qr';
import { RuntimeLogPanel } from '../../features/runtime-log';
import {
  buildUpdateServerSettingsInput,
  createDraftFromServerSettings,
  createEmptyServerSettingsDraft,
  getServerSettings,
  resolveServerSettingsFieldErrors,
  ServerSettingsModal,
  type ServerSettingsFieldErrors,
  type ServerSettingsDraft,
  updateServerSettings,
  validateServerSettingsDraft,
} from '../../features/server-settings';
import { useI18n } from '../../shared/i18n';
import { pickFilePath } from '../../shared/lib/tauri';
import type { LogEntryDto, RoomPreviewDto, RuntimeLogsChangedDto } from '../../shared/type';
import { Button, Icon } from '../../shared/ui';
import './studio.css';

type RoomListViewMode = 'cards' | 'list';

type EditorState =
  | { mode: 'create' }
  | {
      mode: 'edit';
      room: RoomCardView;
    };

type QrSelectionState = {
  room: RoomCardView;
  linkIndex: number;
};

const ROOM_FORM_ID = 'studio-room-form';
const SERVER_SETTINGS_FORM_ID = 'studio-server-settings-form';
const MAX_RUNTIME_LOGS = 1_024;
const ROOM_LIST_VIEW_STORAGE_KEY = 'studio.room-list-view';

function describeError(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }

  return 'common.requestFailed';
}

function mergeRoomPreviewUpdates(currentRooms: RoomCardView[], nextPreviews: RoomPreviewDto[]) {
  if (nextPreviews.length === 0) {
    return currentRooms;
  }

  const previewsByRoomId = new Map(nextPreviews.map(preview => [preview.room_id, preview]));

  return currentRooms.map(room => {
    const preview = previewsByRoomId.get(room.room.room.id);
    if (!preview) {
      return room;
    }

    return {
      ...room,
      room: {
        ...room.room,
        latest_snapshot: preview.latest_snapshot,
      },
      preview_data_url: preview.preview_data_url,
    };
  });
}

function mergeRuntimeLogs(
  currentLogs: LogEntryDto[],
  payload: RuntimeLogsChangedDto,
) {
  const mergedLogs = payload.replace ? payload.logs : [...currentLogs, ...payload.logs];
  return mergedLogs.slice(-MAX_RUNTIME_LOGS);
}

function compareIsoDateTime(left: string | null, right: string | null) {
  if (!left && !right) {
    return 0;
  }
  if (!left) {
    return -1;
  }
  if (!right) {
    return 1;
  }

  const leftTime = Date.parse(left);
  const rightTime = Date.parse(right);
  if (Number.isNaN(leftTime) || Number.isNaN(rightTime)) {
    return left.localeCompare(right);
  }

  return leftTime - rightTime;
}

function mergeInitialRuntimeLogs(currentLogs: LogEntryDto[], initialLogs: LogEntryDto[]) {
  if (initialLogs.length === 0) {
    return currentLogs;
  }

  const seen = new Set(
    currentLogs.map(log => `${log.at}:${log.level}:${log.scope}:${log.message}`),
  );
  const mergedLogs = [...currentLogs];

  for (const log of initialLogs) {
    const key = `${log.at}:${log.level}:${log.scope}:${log.message}`;
    if (seen.has(key)) {
      continue;
    }

    seen.add(key);
    mergedLogs.push(log);
  }

  mergedLogs.sort((left, right) => compareIsoDateTime(left.at, right.at));
  return mergedLogs.slice(-MAX_RUNTIME_LOGS);
}

function latestRuntimeGeneratedAt(current: string | null, next: string | null) {
  return compareIsoDateTime(current, next) >= 0 ? current : next;
}

function readPersistedRoomListViewMode(): RoomListViewMode {
  if (typeof window === 'undefined') {
    return 'cards';
  }

  try {
    const storedValue = window.localStorage.getItem(ROOM_LIST_VIEW_STORAGE_KEY);
    return storedValue === 'list' ? 'list' : 'cards';
  } catch {
    return 'cards';
  }
}

function applyCreateDraftNameFallback(draft: RoomFormDraft): RoomFormDraft {
  if (draft.name.trim() || !draft.target_path.trim()) {
    return draft;
  }

  const derivedName = deriveRoomNameFromTargetPath(draft.target_path);
  if (!derivedName) {
    return draft;
  }

  return {
    ...draft,
    name: derivedName,
  };
}

export function StudioPage() {
  const { t, translateMaybe } = useI18n();
  const [rooms, setRooms] = useState<RoomCardView[]>([]);
  const [roomListViewMode, setRoomListViewMode] = useState<RoomListViewMode>(
    () => readPersistedRoomListViewMode(),
  );
  const [expandedRoomId, setExpandedRoomId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [pageError, setPageError] = useState<string | null>(null);
  const [editorState, setEditorState] = useState<EditorState | null>(null);
  const [draft, setDraft] = useState<RoomFormDraft>(createEmptyDraft());
  const [editorFieldErrors, setEditorFieldErrors] = useState<RoomFormFieldErrors>({});
  const [editorError, setEditorError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [pickingTargetPath, setPickingTargetPath] = useState(false);
  const [deleteTargetRoom, setDeleteTargetRoom] = useState<RoomCardView | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deletingRoom, setDeletingRoom] = useState(false);
  const [qrSelection, setQrSelection] = useState<QrSelectionState | null>(null);
  const [runtimeLogs, setRuntimeLogs] = useState<LogEntryDto[]>([]);
  const [runtimeGeneratedAt, setRuntimeGeneratedAt] = useState<string | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [serverSettingsOpen, setServerSettingsOpen] = useState(false);
  const [serverSettingsDraft, setServerSettingsDraft] = useState<ServerSettingsDraft>(
    createEmptyServerSettingsDraft(),
  );
  const [serverSettingsFieldErrors, setServerSettingsFieldErrors] =
    useState<ServerSettingsFieldErrors>({});
  const [serverSettingsLoading, setServerSettingsLoading] = useState(false);
  const [serverSettingsSubmitting, setServerSettingsSubmitting] = useState(false);
  const [serverSettingsError, setServerSettingsError] = useState<string | null>(null);

  const refreshRooms = async (showSpinner = false) => {
    if (showSpinner) {
      setLoading(true);
    }

    try {
      const nextRooms = await listRooms();
      setRooms(nextRooms);
      setPageError(null);
    } catch (error) {
      setPageError(describeError(error));
    } finally {
      if (showSpinner) {
        setLoading(false);
      }
    }
  };

  useEffect(() => {
    void refreshRooms(true);

    let disposed = false;
    let unlistenRoomsChanged: (() => void) | null = null;
    let unlistenRoomPreviewsChanged: (() => void) | null = null;
    let unlistenRuntimeLogsChanged: (() => void) | null = null;

    void (async () => {
      try {
        unlistenRoomsChanged = await listenRoomsChanged(nextRooms => {
          if (disposed) {
            return;
          }

          startTransition(() => {
            setRooms(nextRooms);
            setPageError(null);
          });
        });
        unlistenRoomPreviewsChanged = await listenRoomPreviewsChanged(nextPreviews => {
          if (disposed) {
            return;
          }

          startTransition(() => {
            setRooms(currentRooms => mergeRoomPreviewUpdates(currentRooms, nextPreviews));
            setPageError(null);
          });
        });
        unlistenRuntimeLogsChanged = await listenRuntimeLogsChanged(nextPayload => {
          if (disposed) {
            return;
          }

          startTransition(() => {
            setRuntimeLogs(currentLogs => mergeRuntimeLogs(currentLogs, nextPayload));
            setRuntimeGeneratedAt(current =>
              latestRuntimeGeneratedAt(current, nextPayload.generated_at),
            );
            setRuntimeError(null);
          });
        });

        const status = await getServerStatus();
        if (disposed) {
          return;
        }

        startTransition(() => {
          setRuntimeLogs(currentLogs => mergeInitialRuntimeLogs(currentLogs, status.logs));
          setRuntimeGeneratedAt(current =>
            latestRuntimeGeneratedAt(current, status.generated_at),
          );
          setRuntimeError(null);
        });
      } catch (error) {
        if (disposed) {
          return;
        }

        const message = describeError(error);
        setPageError(current => current ?? message);
        setRuntimeError(current => current ?? message);
      }
    })();

    return () => {
      disposed = true;
      unlistenRoomsChanged?.();
      unlistenRoomPreviewsChanged?.();
      unlistenRuntimeLogsChanged?.();
    };
  }, []);

  useEffect(() => {
    if (!expandedRoomId) {
      return;
    }

    if (rooms.some(room => room.room.room.id === expandedRoomId)) {
      return;
    }

    setExpandedRoomId(null);
  }, [expandedRoomId, rooms]);

  useEffect(() => {
    if (!qrSelection) {
      return;
    }

    const nextRoom = rooms.find(room => room.room.room.id === qrSelection.room.room.room.id);
    if (!nextRoom || !nextRoom.viewer_links[qrSelection.linkIndex]) {
      setQrSelection(null);
      return;
    }

    if (nextRoom !== qrSelection.room) {
      setQrSelection({
        room: nextRoom,
        linkIndex: qrSelection.linkIndex,
      });
    }
  }, [qrSelection, rooms]);

  useEffect(() => {
    try {
      window.localStorage.setItem(ROOM_LIST_VIEW_STORAGE_KEY, roomListViewMode);
    } catch {
      // Ignore storage failures and keep the in-memory selection.
    }
  }, [roomListViewMode]);

  const openCreateModal = () => {
    setEditorState({ mode: 'create' });
    setDraft(createEmptyDraft());
    setEditorFieldErrors({});
    setEditorError(null);
  };

  const openEditModal = (room: RoomCardView) => {
    setEditorState({ mode: 'edit', room });
    setDraft(createDraftFromRoom(room));
    setEditorFieldErrors({});
    setEditorError(null);
  };

  const closeEditor = () => {
    if (submitting || pickingTargetPath) {
      return;
    }

    setEditorState(null);
    setEditorFieldErrors({});
    setEditorError(null);
  };

  const handlePickTargetPath = async () => {
    setEditorFieldErrors(current => ({ ...current, name: undefined, target_path: undefined }));
    setEditorError(null);
    setPickingTargetPath(true);

    try {
      const selectedPath = await pickFilePath();
      if (!selectedPath) {
        return;
      }

      setDraft(current => {
        const nextDraft = {
          ...current,
          target_path: selectedPath,
        };

        if (editorState?.mode !== 'create' || current.name.trim()) {
          return nextDraft;
        }

        return {
          ...nextDraft,
          name: deriveRoomNameFromTargetPath(selectedPath),
        };
      });
    } catch (error) {
      setEditorError(describeError(error));
    } finally {
      setPickingTargetPath(false);
    }
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const normalizedDraft =
      editorState?.mode === 'create' ? applyCreateDraftNameFallback(draft) : draft;

    if (normalizedDraft !== draft) {
      setDraft(normalizedDraft);
    }

    const validationErrors = validateRoomDraft(normalizedDraft);
    if (Object.keys(validationErrors).length > 0) {
      setEditorFieldErrors(validationErrors);
      setEditorError(validationErrors.target_path ?? null);
      return;
    }

    setSubmitting(true);
    setEditorFieldErrors({});
    setEditorError(null);

    try {
      if (editorState?.mode === 'edit') {
        await updateRoom(editorState.room.room.room.id, buildUpdateRoomInput(normalizedDraft));
      } else {
        await createRoom(buildCreateRoomInput(normalizedDraft));
      }

      setEditorState(null);
    } catch (error) {
      const message = describeError(error);
      const fieldErrors = resolveRoomFormFieldErrors(message);

      if (Object.keys(fieldErrors).length > 0) {
        setEditorFieldErrors(fieldErrors);
        setEditorError(fieldErrors.target_path ?? null);
      } else {
        setEditorError(message);
      }
    } finally {
      setSubmitting(false);
    }
  };

  const handleToggleRunning = async (room: RoomCardView) => {
    try {
      await setRoomRunning(room.room.room.id, room.room.state !== 'running');
    } catch (error) {
      setPageError(describeError(error));
    }
  };

  const openServerSettings = async () => {
    setServerSettingsOpen(true);
    setServerSettingsLoading(true);
    setServerSettingsFieldErrors({});
    setServerSettingsError(null);

    try {
      const settings = await getServerSettings();
      setServerSettingsDraft(createDraftFromServerSettings(settings));
    } catch (error) {
      setServerSettingsError(describeError(error));
    } finally {
      setServerSettingsLoading(false);
    }
  };

  const closeServerSettings = () => {
    if (serverSettingsLoading || serverSettingsSubmitting) {
      return;
    }

    setServerSettingsOpen(false);
    setServerSettingsFieldErrors({});
    setServerSettingsError(null);
  };

  const handleServerSettingsSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const validationErrors = validateServerSettingsDraft(serverSettingsDraft);
    if (Object.keys(validationErrors).length > 0) {
      setServerSettingsFieldErrors(validationErrors);
      setServerSettingsError(null);
      return;
    }

    setServerSettingsSubmitting(true);
    setServerSettingsFieldErrors({});
    setServerSettingsError(null);

    try {
      await updateServerSettings(buildUpdateServerSettingsInput(serverSettingsDraft));
      await refreshRooms();
      setServerSettingsOpen(false);
    } catch (error) {
      const message = describeError(error);
      const fieldErrors = resolveServerSettingsFieldErrors(message);

      if (Object.keys(fieldErrors).length > 0) {
        setServerSettingsFieldErrors(fieldErrors);
        setServerSettingsError(null);
      } else {
        setServerSettingsError(message);
      }
    } finally {
      setServerSettingsSubmitting(false);
    }
  };

  const closeDeleteModal = () => {
    if (deletingRoom) {
      return;
    }

    setDeleteTargetRoom(null);
    setDeleteError(null);
  };

  const handleDelete = (room: RoomCardView) => {
    setDeleteTargetRoom(room);
    setDeleteError(null);
  };

  const handleConfirmDelete = async () => {
    if (!deleteTargetRoom) {
      return;
    }

    setDeletingRoom(true);
    setDeleteError(null);

    try {
      await deleteRoom(deleteTargetRoom.room.room.id);
      setDeleteTargetRoom(null);
    } catch (error) {
      setDeleteError(describeError(error));
    } finally {
      setDeletingRoom(false);
    }
  };

  const editorTitle =
    editorState?.mode === 'edit' ? t('common.editRoom') : t('common.createRoom');

  return (
    <main className="studio-dashboard">
      <header className="studio-dashboard__header">
        <div className="studio-dashboard__view-toggle" role="group" aria-label={t('studio.roomViewAria')}>
          <Button
            className="studio-dashboard__action studio-dashboard__view-button"
            variant={roomListViewMode === 'cards' ? 'primary' : 'secondary'}
            size="sm"
            aria-pressed={roomListViewMode === 'cards'}
            onClick={() => setRoomListViewMode('cards')}
          >
            <Icon name="grid" size="xs" aria-hidden />
            <span>{t('common.cards')}</span>
          </Button>
          <Button
            className="studio-dashboard__action studio-dashboard__view-button"
            variant={roomListViewMode === 'list' ? 'primary' : 'secondary'}
            size="sm"
            aria-pressed={roomListViewMode === 'list'}
            onClick={() => setRoomListViewMode('list')}
          >
            <Icon name="list" size="xs" aria-hidden />
            <span>{t('common.list')}</span>
          </Button>
        </div>
        <div className="studio-dashboard__header-actions">
          <Button
            className="studio-dashboard__action"
            variant="secondary"
            size="sm"
            onClick={() => void openServerSettings()}
          >
            <Icon name="setting" size="xs" aria-hidden />
            <span>{t('studio.serverButton')}</span>
          </Button>
          <Button className="studio-dashboard__action" size="sm" onClick={openCreateModal}>
            <Icon name="plus" size="xs" aria-hidden />
            <span>{t('studio.roomButton')}</span>
          </Button>
        </div>
      </header>

      {pageError ? (
        <section className="studio-dashboard__notice" aria-live="polite">
          <p>{translateMaybe(pageError)}</p>
          <Button variant="secondary" size="sm" onClick={() => void refreshRooms(true)}>
            {t('common.retry')}
          </Button>
        </section>
      ) : null}

      {loading ? (
        <section className="studio-dashboard__empty" aria-live="polite">
          <Icon name="reload" size="lg" aria-hidden />
          <p>{t('studio.loadingRooms')}</p>
        </section>
      ) : rooms.length === 0 ? (
        <section className="studio-dashboard__empty" aria-live="polite">
          <Icon name="grid" size="lg" aria-hidden />
          <p>{t('studio.noRooms')}</p>
          <Button size="sm" onClick={openCreateModal}>
            {t('studio.createFirstRoom')}
          </Button>
        </section>
      ) : roomListViewMode === 'cards' ? (
        <section className="studio-dashboard__grid" aria-label={t('studio.roomCardsAria')}>
          {rooms.map(room => (
            <RoomCard
              key={room.room.room.id}
              {...toRoomCardProps(room)}
              onEdit={() => openEditModal(room)}
              onToggleRunning={() => void handleToggleRunning(room)}
              onDelete={() => handleDelete(room)}
              onShowQr={
                room.viewer_links.length > 0
                  ? linkIndex => setQrSelection({ room, linkIndex })
                  : undefined
              }
            />
          ))}
        </section>
      ) : (
        <section className="studio-dashboard__list" aria-label={t('studio.roomListAria')}>
          <RoomAccordionList
            rooms={rooms}
            expandedRoomId={expandedRoomId}
            onExpandedRoomIdChange={setExpandedRoomId}
            onEdit={openEditModal}
            onToggleRunning={room => void handleToggleRunning(room)}
            onDelete={handleDelete}
            onShowQr={(room, linkIndex) => setQrSelection({ room, linkIndex })}
          />
        </section>
      )}

      <RuntimeLogPanel
        logs={runtimeLogs}
        runtimeError={runtimeError}
        runtimeGeneratedAt={runtimeGeneratedAt}
      />

      <ServerSettingsModal
        open={serverSettingsOpen}
        formId={SERVER_SETTINGS_FORM_ID}
        draft={serverSettingsDraft}
        fieldErrors={serverSettingsFieldErrors}
        loading={serverSettingsLoading}
        submitting={serverSettingsSubmitting}
        error={serverSettingsError}
        onClose={closeServerSettings}
        onSubmit={handleServerSettingsSubmit}
        onDraftChange={updater => {
          setServerSettingsDraft(current => updater(current));
          setServerSettingsFieldErrors({});
          setServerSettingsError(null);
        }}
      />

      <RoomEditorModal
        open={editorState !== null}
        title={editorTitle}
        submitLabel={
          editorState?.mode === 'edit' ? t('common.saveChanges') : t('common.createRoom')
        }
        formId={ROOM_FORM_ID}
        draft={draft}
        fieldErrors={editorFieldErrors}
        editorError={editorError}
        submitting={submitting}
        pickingTargetPath={pickingTargetPath}
        onClose={closeEditor}
        onSubmit={handleSubmit}
        onPickTargetPath={() => void handlePickTargetPath()}
        onDraftChange={updater => {
          setDraft(current => updater(current));
          setEditorFieldErrors({});
          setEditorError(null);
        }}
      />

      <RoomDeleteModal
        room={deleteTargetRoom}
        deleting={deletingRoom}
        error={deleteError}
        onClose={closeDeleteModal}
        onConfirm={() => void handleConfirmDelete()}
      />

      <RoomQrModal
        room={qrSelection?.room ?? null}
        linkIndex={qrSelection?.linkIndex ?? null}
        onClose={() => setQrSelection(null)}
      />
    </main>
  );
}
