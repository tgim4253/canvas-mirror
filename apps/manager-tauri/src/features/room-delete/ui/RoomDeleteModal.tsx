import { getPrimaryViewerUrl, type RoomCardView } from '../../../entities/room/model';
import { useI18n } from '../../../shared/i18n';
import { cx } from '../../../shared/lib/cx';
import { Button, Chip, Icon, Modal, ModalBody, ModalFooter, ModalHeader } from '../../../shared/ui';
import './room-delete-modal.css';

type RoomDeleteModalProps = {
  room: RoomCardView | null;
  deleting: boolean;
  error: string | null;
  onClose: () => void;
  onConfirm: () => void;
};

const ROOM_STATUS_LABEL_KEYS: Record<RoomCardView['room']['state'], string> = {
  running: 'common.running',
  paused: 'common.paused',
  error: 'common.error',
};

export function RoomDeleteModal({
  room,
  deleting,
  error,
  onClose,
  onConfirm,
}: RoomDeleteModalProps) {
  const { t, translateMaybe } = useI18n();
  const viewerUrl = room ? getPrimaryViewerUrl(room) : null;

  return (
    <Modal
      open={room !== null}
      size="sm"
      onClose={deleting ? undefined : onClose}
      header={
        <ModalHeader
          title={
            <span className="room-delete-modal__title">
              <Icon name="trash" size="sm" aria-hidden />
              <span>{t('room.delete.title')}</span>
            </span>
          }
          closeButtonLabel={t('common.closeModal')}
          onClose={deleting ? undefined : onClose}
        />
      }
      body={
        room ? (
          <ModalBody className="room-delete-modal">
            <p className="room-delete-modal__copy">
              {t('room.delete.copy', { name: room.room.room.name })}
            </p>

            <div className="room-delete-modal__meta">
              <Chip
                shape="pill"
                variant="outline"
                className={cx(
                  'room-delete-modal__status',
                  `room-delete-modal__status--${room.room.state}`,
                )}
              >
                <span className="room-delete-modal__status-dot" />
                <span>{t(ROOM_STATUS_LABEL_KEYS[room.room.state])}</span>
              </Chip>
              <p className="room-delete-modal__path">{room.target_path}</p>
              <p className="room-delete-modal__relay">{viewerUrl ?? t('common.viewerUnavailable')}</p>
            </div>

            <p className="room-delete-modal__note">{t('room.delete.note')}</p>
            {error ? <p className="room-delete-modal__error">{translateMaybe(error)}</p> : null}
          </ModalBody>
        ) : undefined
      }
      footer={
        <ModalFooter>
          <Button variant="ghost" onClick={onClose} disabled={deleting}>
            {t('common.cancel')}
          </Button>
          <Button variant="destructive" onClick={onConfirm} disabled={deleting}>
            <Icon name="trash" size="xs" aria-hidden />
            <span>{t(deleting ? 'room.delete.deleting' : 'room.delete.title')}</span>
          </Button>
        </ModalFooter>
      }
    />
  );
}
