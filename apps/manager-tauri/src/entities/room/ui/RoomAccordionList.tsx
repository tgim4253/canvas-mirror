import type { ReactNode } from 'react';
import { cx } from '../../../shared/lib/cx';
import { useI18n } from '../../../shared/i18n';
import { toDocumentLang } from '../../../shared/i18n/resources';
import type { OutputResolution, RoomState } from '../../../shared/type';
import {
  AccordionDisclosure,
  AccordionItem,
  AccordionItemBody,
  AccordionItemHeader,
  AccordionRoot,
  Button,
  Chip,
  Icon,
} from '../../../shared/ui';
import { getVisibleViewerLinks, type RoomCardView } from '../model';
import './room-accordion-list.css';

type RoomAccordionListProps = {
  rooms: RoomCardView[];
  expandedRoomId: string | null;
  onExpandedRoomIdChange: (roomId: string | null) => void;
  onEdit?: (room: RoomCardView) => void;
  onToggleRunning?: (room: RoomCardView) => void;
  onDelete?: (room: RoomCardView) => void;
  onShowQr?: (room: RoomCardView, linkIndex: number) => void;
};

const STATUS_LABEL_KEYS: Record<RoomState, string> = {
  running: 'common.running',
  paused: 'common.paused',
  error: 'common.error',
};

function formatResolution(
  resolution: OutputResolution,
  t: (key: string, values?: Record<string, string | number>) => string,
) {
  if (resolution.kind === 'source') {
    return t('roomForm.resolution.source');
  }

  return t('roomAccordion.resolution.contain', {
    width: resolution.max_width,
    height: resolution.max_height,
  });
}

function formatTrigger(
  room: RoomCardView,
  t: (key: string, values?: Record<string, string | number>) => string,
) {
  const summary = room.room.room;

  if (!summary.detection_enabled) {
    return t('roomAccordion.trigger.detectionOff');
  }

  if (summary.mode === 'interval') {
    return t('roomAccordion.trigger.interval', {
      interval: summary.interval_ms,
    });
  }

  return t('roomAccordion.trigger.watch', {
    debounce: summary.debounce_ms,
    stabilize: summary.stabilize_ms,
  });
}

function formatBytes(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }

  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatSnapshot(
  room: RoomCardView,
  t: (key: string, values?: Record<string, string | number>) => string,
) {
  const snapshot = room.room.latest_snapshot;

  if (!snapshot) {
    return t('roomAccordion.snapshot.none');
  }

  const dimension =
    snapshot.width && snapshot.height
      ? `${snapshot.width}×${snapshot.height}`
      : t('roomAccordion.snapshot.unknownSize');

  return `${snapshot.mime_type} · ${dimension} · ${formatBytes(snapshot.bytes_len)}`;
}

function formatTimestamp(
  value: string | null,
  locale: string,
  t: (key: string, values?: Record<string, string | number>) => string,
) {
  if (!value) {
    return t('roomAccordion.timestamp.never');
  }

  const timestamp = new Date(value);
  if (Number.isNaN(timestamp.getTime())) {
    return value;
  }

  return timestamp.toLocaleString(locale);
}

function RoomPreview({
  room,
  previewAlt,
}: {
  room: RoomCardView;
  previewAlt: string;
}) {
  const { locale, t } = useI18n();
  const intlLocale = toDocumentLang(locale);
  const updatedLabel = t('runtime.updated', {
    time: formatTimestamp(room.room.latest_snapshot?.created_at ?? null, intlLocale, t),
  });

  return (
    <div className="room-accordion-list__preview-frame">
      {room.preview_data_url ? (
        <img
          className="room-accordion-list__preview-image"
          src={room.preview_data_url}
          alt={previewAlt}
          loading="lazy"
          decoding="async"
        />
      ) : (
        <div
          className="room-accordion-list__preview-placeholder"
          aria-label={t('room.placeholder.noImageAria')}
        >
          <Icon name="camera" size="lg" hierarchy="tertiary" aria-hidden />
          <span className="room-accordion-list__preview-placeholder-copy">
            {t('room.placeholder.noImage')}
          </span>
        </div>
      )}

      <div className="room-accordion-list__preview-meta">
        <span className="room-accordion-list__preview-device-count">
          <Icon name="smartphone" size="xs" aria-hidden />
          <span>{room.room.devices.length}</span>
        </span>
        <span className="room-accordion-list__preview-updated">{updatedLabel}</span>
      </div>
    </div>
  );
}

function RoomDetail({
  label,
  value,
  tone = 'default',
}: {
  label: string;
  value: ReactNode;
  tone?: 'default' | 'danger';
}) {
  return (
    <div
      className={cx(
        'room-accordion-list__detail',
        tone === 'danger' && 'room-accordion-list__detail--danger',
      )}
    >
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

function ViewerUrls({
  room,
  roomName,
}: {
  room: RoomCardView;
  roomName: string;
}) {
  const { t } = useI18n();
  const viewerLinks = getVisibleViewerLinks(room);

  if (viewerLinks.length === 0) {
    return <span>{t('common.viewerUnavailable')}</span>;
  }

  return (
    <div className="room-accordion-list__viewer-links">
      {viewerLinks.map((viewerLink, index) => (
        <div
          key={`${viewerLink.index}-${viewerLink.viewer_url}`}
          className="room-accordion-list__viewer-link"
        >
          <span
            className="room-accordion-list__viewer-link-index"
            aria-label={`${roomName} viewer ${index + 1}`}
          >
            {index + 1}
          </span>
          <span className="room-accordion-list__viewer-link-value">{viewerLink.viewer_url}</span>
        </div>
      ))}
    </div>
  );
}

export function RoomAccordionList({
  rooms,
  expandedRoomId,
  onExpandedRoomIdChange,
  onEdit,
  onToggleRunning,
  onDelete,
  onShowQr,
}: RoomAccordionListProps) {
  const { t } = useI18n();

  return (
    <AccordionRoot
      type="single"
      value={expandedRoomId}
      onValueChange={nextValue =>
        onExpandedRoomIdChange(typeof nextValue === 'string' ? nextValue : null)
      }
      className="room-accordion-list"
    >
      {rooms.map((room, index) => {
        const roomId = room.room.room.id;
        const name = room.room.room.name;
        const status = room.room.state;
        const isRunning = status === 'running';
        const previewAlt = t('room.previewAlt', { name });
        const viewerLinks = getVisibleViewerLinks(room);

        return (
          <AccordionItem
            key={roomId}
            value={roomId}
            className={cx(
              'room-accordion-list__item',
              `room-accordion-list__item--${status}`,
            )}
          >
            <div className="room-accordion-list__header-row">
              <AccordionItemHeader
                index={String(index + 1).padStart(2, '0')}
                className="room-accordion-list__header"
                trailing={
                  <span className="room-accordion-list__header-trailing">
                    <span className="room-accordion-list__header-device-count">
                      <Icon name="smartphone" size="xs" aria-hidden />
                      <span>{room.room.devices.length}</span>
                    </span>
                    <Chip
                      shape="pill"
                      variant="outline"
                      className={cx(
                        'room-accordion-list__status',
                        `room-accordion-list__status--${status}`,
                      )}
                    >
                      <span className="room-accordion-list__status-dot" />
                      <span>{t(STATUS_LABEL_KEYS[status])}</span>
                    </Chip>

                    <AccordionDisclosure>{t('common.inspect')}</AccordionDisclosure>
                  </span>
                }
              >
                <span className="room-accordion-list__header-copy">
                  <span className="room-accordion-list__header-title">{name}</span>
                  <span className="room-accordion-list__header-path">{room.target_path}</span>
                </span>
              </AccordionItemHeader>
            </div>

            <AccordionItemBody keepMounted={false} className="room-accordion-list__body">
              <div className="room-accordion-list__body-grid">
                <div className="room-accordion-list__preview-panel">
                  <RoomPreview room={room} previewAlt={previewAlt} />
                </div>

                <div className="room-accordion-list__settings-panel">
                  <dl className="room-accordion-list__detail-grid">
                    <RoomDetail label={t('common.target')} value={room.target_path} />
                    <RoomDetail
                      label={t('common.viewer')}
                      value={<ViewerUrls room={room} roomName={name} />}
                    />
                    <RoomDetail label={t('common.trigger')} value={formatTrigger(room, t)} />
                    <RoomDetail
                      label={t('roomEditor.resolution')}
                      value={formatResolution(room.room.room.resolution, t)}
                    />
                    <RoomDetail label={t('common.snapshot')} value={formatSnapshot(room, t)} />
                    <RoomDetail
                      label={t('common.lastError')}
                      value={room.room.last_error ?? t('common.none')}
                      tone={room.room.last_error ? 'danger' : 'default'}
                    />
                  </dl>

                  <div className="room-accordion-list__actions">
                    <Button
                      variant="secondary"
                      size="sm"
                      className="room-accordion-list__action"
                      onClick={onEdit ? () => onEdit(room) : undefined}
                      disabled={!onEdit}
                    >
                      <Icon name="setting" size="xs" aria-hidden />
                      <span>{t('common.edit')}</span>
                    </Button>
                    <Button
                      variant="secondary"
                      size="sm"
                      className="room-accordion-list__action"
                      onClick={onToggleRunning ? () => onToggleRunning(room) : undefined}
                      disabled={!onToggleRunning}
                    >
                      <Icon name={isRunning ? 'pause' : 'play'} size="xs" aria-hidden />
                      <span>{t(isRunning ? 'common.pause' : 'common.resume')}</span>
                    </Button>
                    {viewerLinks.map((viewerLink, linkIndex) => (
                      <Button
                        key={`${viewerLink.index}-${viewerLink.viewer_url}`}
                        variant="secondary"
                        size="sm"
                        className="room-accordion-list__action"
                        onClick={
                          onShowQr && viewerLink.qr_svg
                            ? () => onShowQr(room, viewerLink.index)
                            : undefined
                        }
                        disabled={!onShowQr || !viewerLink.qr_svg}
                      >
                        <Icon name="qr-code" size="xs" aria-hidden />
                        <span>{`${t('common.qr')} ${linkIndex + 1}`}</span>
                      </Button>
                    ))}
                    <Button
                      variant="destructive"
                      size="sm"
                      className="room-accordion-list__action"
                      onClick={onDelete ? () => onDelete(room) : undefined}
                      disabled={!onDelete}
                    >
                      <Icon name="trash" size="xs" aria-hidden />
                      <span>{t('common.delete')}</span>
                    </Button>
                  </div>
                </div>
              </div>
            </AccordionItemBody>
          </AccordionItem>
        );
      })}
    </AccordionRoot>
  );
}
