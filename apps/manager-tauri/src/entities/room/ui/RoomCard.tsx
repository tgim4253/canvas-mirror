import { cx } from '../../../shared/lib/cx';
import { useI18n } from '../../../shared/i18n';
import { ROOM_STATES, type RoomState } from '../../../shared/type';
import { Button, Chip, Icon, IconButton } from '../../../shared/ui';
import './room-card.css';

export const ROOM_CARD_STATUSES = ROOM_STATES;

export type RoomCardStatus = RoomState;

export type RoomCardProps = {
  name: string;
  sourcePath: string;
  previewUrl?: string;
  previewAlt?: string;
  viewerLinks: Array<{
    viewerUrl: string;
    hasQr: boolean;
    sourceIndex: number;
  }>;
  deviceCount: number;
  status: RoomCardStatus;
  onEdit?: () => void;
  onToggleRunning?: () => void;
  onDelete?: () => void;
  onShowQr?: (index: number) => void;
};

const STATUS_LABEL_KEYS: Record<RoomCardStatus, string> = {
  running: 'common.running',
  paused: 'common.paused',
  error: 'common.error',
};

export function RoomCard({
  name,
  sourcePath,
  previewUrl,
  previewAlt,
  viewerLinks,
  deviceCount,
  status,
  onEdit,
  onToggleRunning,
  onDelete,
  onShowQr,
}: RoomCardProps) {
  const { t } = useI18n();
  const isRunning = status === 'running';
  const toggleIcon = isRunning ? 'pause' : 'play';
  const resolvedPreviewAlt = previewAlt ?? t('room.previewAlt', { name });
  const toggleLabel = isRunning
    ? t('room.action.stop', { name })
    : t('room.action.start', { name });

  return (
    <article className={`room-card room-card--${status}`}>
      <header className="room-card__header">
        <div className="room-card__header-row">
          <div className="room-card__identity">
            <h3 className="room-card__title">{name}</h3>
            <Chip
              shape="pill"
              variant="outline"
              className={cx('room-card__status', `room-card__status--${status}`)}
            >
              <span className="room-card__status-dot" />
              <span>{t(STATUS_LABEL_KEYS[status])}</span>
            </Chip>
          </div>
          <div className="room-card__actions">
            <IconButton
              className="room-card__edit"
              icon="setting"
              size="lg"
              iconSize="md"
              aria-label={t('room.action.edit', { name })}
              onClick={onEdit}
            />
            <IconButton
              className={cx('room-card__toggle', `room-card__toggle--${status}`)}
              icon={toggleIcon}
              size="lg"
              iconSize="md"
              aria-label={toggleLabel}
              onClick={onToggleRunning}
            />
            <IconButton
              className="room-card__delete"
              icon="trash"
              size="lg"
              iconSize="md"
              aria-label={t('room.action.delete', { name })}
              onClick={onDelete}
            />
          </div>
        </div>
        <p className="room-card__path">{sourcePath}</p>
      </header>

      <div className="room-card__snapshot">
        {previewUrl ? (
          <img className="room-card__image" src={previewUrl} alt={resolvedPreviewAlt} />
        ) : (
          <div className="room-card__placeholder" aria-label={t('room.placeholder.noImageAria')}>
            <Icon name="camera" size="lg" hierarchy="tertiary" aria-hidden />
            <span className="room-card__placeholder-copy">{t('room.placeholder.noImage')}</span>
          </div>
        )}
        <div className="room-card__device-chip">
          <Icon name="smartphone" size="xs" aria-hidden />
          <span>{deviceCount}</span>
        </div>
      </div>

      <div className="room-card__body">
        {viewerLinks.length > 0 ? (
          <div className="room-card__relay-list">
            {viewerLinks.map((viewerLink, index) => (
              <Button
                key={viewerLink.viewerUrl}
                variant="secondary"
                width="fill"
                className="room-card__relay"
                aria-label={`${t('room.action.showQr', { name })} ${index + 1}`}
                disabled={!onShowQr || !viewerLink.hasQr}
                onClick={
                  onShowQr && viewerLink.hasQr
                    ? () => onShowQr(viewerLink.sourceIndex)
                    : undefined
                }
              >
                <span className="room-card__relay-order">{index + 1}</span>
                <span className="room-card__relay-label">{viewerLink.viewerUrl}</span>
                <span className="room-card__relay-icon">
                  <Icon name="qr-code" size="sm" hierarchy="tertiary" aria-hidden />
                </span>
              </Button>
            ))}
          </div>
        ) : (
          <div className="room-card__relay room-card__relay--static">
            <span className="room-card__relay-label">{t('common.viewerUnavailable')}</span>
          </div>
        )}
      </div>
    </article>
  );
}
