import type { FormEvent } from 'react';
import { useI18n } from '../../../shared/i18n';
import { cx } from '../../../shared/lib/cx';
import type { DetectionMode } from '../../../shared/type';
import {
  Button,
  CheckboxRow,
  Icon,
  Input,
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader,
  Select,
} from '../../../shared/ui';
import {
  MODE_OPTIONS,
  RESOLUTION_OPTIONS,
  RESOLUTION_PRESETS,
  type RoomFormFieldErrors,
  type ResolutionPreset,
  type RoomFormDraft,
} from '../model/roomForm';
import './room-editor-modal.css';

const MODE_TOOLTIP_KEYS: Record<DetectionMode, string> = {
  watch: 'roomEditor.tooltip.mode.watch',
  interval: 'roomEditor.tooltip.mode.interval',
};

type RoomEditorModalProps = {
  open: boolean;
  title: string;
  submitLabel: string;
  formId: string;
  draft: RoomFormDraft;
  fieldErrors: RoomFormFieldErrors;
  editorError: string | null;
  submitting: boolean;
  pickingTargetPath: boolean;
  onClose: () => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onPickTargetPath: () => void;
  onDraftChange: (updater: (current: RoomFormDraft) => RoomFormDraft) => void;
};

export function RoomEditorModal({
  open,
  title,
  submitLabel,
  formId,
  draft,
  fieldErrors,
  editorError,
  submitting,
  pickingTargetPath,
  onClose,
  onSubmit,
  onPickTargetPath,
  onDraftChange,
}: RoomEditorModalProps) {
  const { t, translateMaybe } = useI18n();
  const showIntervalField = draft.mode === 'interval';
  const showWatchFields = draft.mode === 'watch';
  const showCustomResolutionFields = draft.resolution_preset === 'custom';
  const modeOptions = MODE_OPTIONS.map(option => ({
    value: option.value,
    label: t(option.labelKey),
  }));
  const resolutionOptions = RESOLUTION_OPTIONS.map(option => ({
    value: option.value,
    label: t(option.labelKey),
  }));

  return (
    <Modal
      open={open}
      size="lg"
      onClose={onClose}
      header={<ModalHeader title={title} closeButtonLabel={t('common.closeModal')} onClose={onClose} />}
      body={
        <ModalBody className="room-editor-modal__body">
          <form id={formId} className="room-editor-modal__form" onSubmit={onSubmit}>
            <div className="room-editor-modal__grid">
              <Input
                label={t('roomEditor.roomName')}
                title={t('roomEditor.tooltip.roomName')}
                className="room-editor-modal__field room-editor-modal__field--wide"
                error={fieldErrors.name ? translateMaybe(fieldErrors.name) : undefined}
                value={draft.name}
                onChange={event =>
                  onDraftChange(current => ({
                    ...current,
                    name: event.target.value,
                  }))
                }
              />
              <div className="room-editor-modal__field room-editor-modal__field--wide room-editor-modal__file-field">
                <span className="room-editor-modal__label" title={t('roomEditor.tooltip.targetPath')}>
                  {t('roomEditor.targetPath')}
                </span>
                <Button
                  type="button"
                  variant="secondary"
                  width="fill"
                  className="room-editor-modal__file-trigger"
                  title={t('roomEditor.tooltip.targetPath')}
                  disabled={submitting || pickingTargetPath}
                  onClick={onPickTargetPath}
                >
                  <span className="room-editor-modal__file-trigger-copy">
                    <span
                      className={cx(
                        'room-editor-modal__file-trigger-value',
                        !draft.target_path && 'room-editor-modal__file-trigger-value--placeholder',
                      )}
                    >
                      {draft.target_path || t('roomEditor.chooseFile')}
                    </span>
                    <span className="room-editor-modal__file-trigger-hint">
                      {pickingTargetPath
                        ? t('roomEditor.openingFilePicker')
                        : t('roomEditor.clickToBrowse')}
                    </span>
                  </span>
                  <span className="room-editor-modal__file-trigger-icon">
                    <Icon name="folder" size="sm" aria-hidden />
                  </span>
                </Button>
              </div>
              <div className="room-editor-modal__field room-editor-modal__field--wide room-editor-modal__mode-field">
                <span className="room-editor-modal__label" title={t('roomEditor.tooltip.mode')}>
                  {t('roomEditor.detectionMode')}
                </span>
                <div
                  className="room-editor-modal__mode-segmented"
                  role="tablist"
                  aria-label={t('roomEditor.detectionModeAria')}
                  title={t('roomEditor.tooltip.mode')}
                >
                  {modeOptions.map(option => {
                    const isActive = draft.mode === option.value;

                    return (
                      <Button
                        key={option.value}
                        type="button"
                        width="fill"
                        variant={isActive ? 'primary' : 'ghost'}
                        className={cx(
                          'room-editor-modal__mode-button',
                          isActive && 'room-editor-modal__mode-button--active',
                        )}
                        title={t(MODE_TOOLTIP_KEYS[option.value as DetectionMode])}
                        onClick={() =>
                          onDraftChange(current => ({
                            ...current,
                            mode: option.value as DetectionMode,
                          }))
                        }
                      >
                        {option.label}
                      </Button>
                    );
                  })}
                </div>
              </div>
              {showIntervalField ? (
                <Input
                  label={t('roomEditor.intervalMs')}
                  title={t('roomEditor.tooltip.intervalMs')}
                  type="number"
                  min={1}
                  error={fieldErrors.interval_ms ? translateMaybe(fieldErrors.interval_ms) : undefined}
                  value={draft.interval_ms}
                  onChange={event =>
                    onDraftChange(current => ({
                      ...current,
                      interval_ms: event.target.value,
                    }))
                  }
                />
              ) : null}
              {showWatchFields ? (
                <Input
                  label={t('roomEditor.debounceMs')}
                  title={t('roomEditor.tooltip.debounceMs')}
                  type="number"
                  min={0}
                  error={fieldErrors.debounce_ms ? translateMaybe(fieldErrors.debounce_ms) : undefined}
                  value={draft.debounce_ms}
                  onChange={event =>
                    onDraftChange(current => ({
                      ...current,
                      debounce_ms: event.target.value,
                    }))
                  }
                />
              ) : null}
              {showWatchFields ? (
                <Input
                  label={t('roomEditor.stabilizeMs')}
                  title={t('roomEditor.tooltip.stabilizeMs')}
                  type="number"
                  min={0}
                  error={fieldErrors.stabilize_ms ? translateMaybe(fieldErrors.stabilize_ms) : undefined}
                  value={draft.stabilize_ms}
                  onChange={event =>
                    onDraftChange(current => ({
                      ...current,
                      stabilize_ms: event.target.value,
                    }))
                  }
                />
              ) : null}
              <Select
                className="room-editor-modal__field room-editor-modal__field--wide"
                label={t('roomEditor.resolution')}
                title={t('roomEditor.tooltip.resolution')}
                options={resolutionOptions}
                value={draft.resolution_preset}
                onValueChange={value =>
                  onDraftChange(current => {
                    const nextPreset = value as RoomFormDraft['resolution_preset'];

                    if (
                      nextPreset !== 'source' &&
                      nextPreset !== 'custom' &&
                      nextPreset in RESOLUTION_PRESETS
                    ) {
                      const preset =
                        RESOLUTION_PRESETS[
                          nextPreset as Exclude<ResolutionPreset, 'source' | 'custom'>
                        ];

                      return {
                        ...current,
                        resolution_preset: nextPreset,
                        max_width: String(preset.max_width),
                        max_height: String(preset.max_height),
                      };
                    }

                    return {
                      ...current,
                      resolution_preset: nextPreset,
                    };
                  })
                }
              />
              {showCustomResolutionFields ? (
                <>
                  <Input
                    label={t('roomEditor.maxWidth')}
                    title={t('roomEditor.tooltip.maxWidth')}
                    type="number"
                    min={1}
                    error={fieldErrors.max_width ? translateMaybe(fieldErrors.max_width) : undefined}
                    value={draft.max_width}
                    onChange={event =>
                      onDraftChange(current => ({
                        ...current,
                        max_width: event.target.value,
                      }))
                    }
                  />
                  <Input
                    label={t('roomEditor.maxHeight')}
                    title={t('roomEditor.tooltip.maxHeight')}
                    type="number"
                    min={1}
                    error={fieldErrors.max_height ? translateMaybe(fieldErrors.max_height) : undefined}
                    value={draft.max_height}
                    onChange={event =>
                      onDraftChange(current => ({
                        ...current,
                        max_height: event.target.value,
                      }))
                    }
                  />
                </>
              ) : null}
            </div>
            <CheckboxRow
              className="room-editor-modal__checkbox"
              label={t('roomEditor.detectionEnabled')}
              title={t('roomEditor.tooltip.detectionEnabled')}
              checked={draft.detection_enabled}
              onCheckedChange={checked =>
                onDraftChange(current => ({
                  ...current,
                  detection_enabled: checked,
                }))
              }
            />
          </form>
          {editorError ? (
            <p className="room-editor-modal__error">{translateMaybe(editorError)}</p>
          ) : null}
        </ModalBody>
      }
      footer={
        <ModalFooter>
          <Button variant="ghost" onClick={onClose} disabled={submitting || pickingTargetPath}>
            {t('common.cancel')}
          </Button>
          <Button form={formId} type="submit" disabled={submitting || pickingTargetPath}>
            {submitting ? t('common.saving') : submitLabel}
          </Button>
        </ModalFooter>
      }
    />
  );
}
