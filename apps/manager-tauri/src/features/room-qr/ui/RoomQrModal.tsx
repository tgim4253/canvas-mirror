import { useEffect, useState } from 'react';
import type { RoomCardView } from '../../../entities/room/model';
import { useI18n } from '../../../shared/i18n';
import { Button, Input, Modal, ModalBody, ModalFooter, ModalHeader } from '../../../shared/ui';
import './room-qr-modal.css';

type RoomQrModalProps = {
  room: RoomCardView | null;
  linkIndex: number | null;
  onClose: () => void;
};

type CopyState = 'idle' | 'copied' | 'error';

async function copyTextToClipboard(value: string) {
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      // Fall through to the document-based copy path below.
    }
  }

  if (typeof document === 'undefined') {
    return false;
  }

  const textarea = document.createElement('textarea');
  textarea.value = value;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  textarea.style.pointerEvents = 'none';

  document.body.appendChild(textarea);
  textarea.select();
  textarea.setSelectionRange(0, textarea.value.length);

  try {
    return document.execCommand('copy');
  } finally {
    document.body.removeChild(textarea);
  }
}

export function RoomQrModal({ room, linkIndex, onClose }: RoomQrModalProps) {
  const { t } = useI18n();
  const [copyState, setCopyState] = useState<CopyState>('idle');
  const viewerLink =
    room && linkIndex !== null ? room.viewer_links[linkIndex] ?? null : null;
  const viewerUrl = viewerLink?.viewer_url ?? t('common.viewerUnavailable');

  useEffect(() => {
    setCopyState('idle');
  }, [linkIndex, room?.room.room.id, viewerLink?.viewer_url]);

  useEffect(() => {
    if (copyState !== 'copied') {
      return;
    }

    const timeout = window.setTimeout(() => {
      setCopyState('idle');
    }, 1800);

    return () => {
      window.clearTimeout(timeout);
    };
  }, [copyState]);

  const handleCopy = async () => {
    if (!viewerLink?.viewer_url) {
      return;
    }

    const copied = await copyTextToClipboard(viewerLink.viewer_url);
    setCopyState(copied ? 'copied' : 'error');
  };

  return (
    <Modal
      open={room !== null}
      size="md"
      onClose={onClose}
      header={
        <ModalHeader
          title={
            room
              ? `${room.room.room.name} · ${t('common.qr')} ${linkIndex !== null ? linkIndex + 1 : ''}`.trim()
              : t('room.qr.title')
          }
          closeButtonLabel={t('common.closeModal')}
          onClose={onClose}
        />
      }
      body={
        room && viewerLink ? (
          <ModalBody className="room-qr-modal">
            {viewerLink.qr_svg ? (
              <div
                className="room-qr-modal__art"
                dangerouslySetInnerHTML={{ __html: viewerLink.qr_svg }}
              />
            ) : null}
            <Input
              label={t('room.qr.viewerUrl')}
              title={t('room.qr.tooltip.viewerUrl')}
              readOnly
              value={viewerUrl}
              className="room-qr-modal__viewer-url"
              controlClassName="room-qr-modal__viewer-url-control"
              inputClassName="room-qr-modal__viewer-url-field"
              onFocus={event => event.currentTarget.select()}
              onClick={event => event.currentTarget.select()}
              hint={copyState === 'error' ? t('room.qr.copyFailedManual') : undefined}
              trailing={
                <Button
                  variant="ghost"
                  size="sm"
                  className="room-qr-modal__copy-button"
                  disabled={!viewerLink.viewer_url}
                  onClick={() => void handleCopy()}
                >
                  {copyState === 'copied' ? t('common.copied') : t('common.copy')}
                </Button>
              }
            />
          </ModalBody>
        ) : undefined
      }
      footer={
        <ModalFooter>
          <Button variant="ghost" onClick={onClose}>
            {t('common.close')}
          </Button>
        </ModalFooter>
      }
    />
  );
}
