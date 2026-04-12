import type { FormEvent } from "react";
import { useI18n } from "../../../shared/i18n";
import { cx } from "../../../shared/lib/cx";
import type {
  AvailableIccProfileDto,
  DetectionMode,
} from "../../../shared/type";
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
} from "../../../shared/ui";
import {
  MODE_OPTIONS,
  RESOLUTION_OPTIONS,
  RESOLUTION_PRESETS,
  type RoomFormFieldErrors,
  type ResolutionPreset,
  type RoomFormDraft,
} from "../model/roomForm";
import "./room-editor-modal.css";

const MODE_TOOLTIP_KEYS: Record<DetectionMode, string> = {
  watch: "roomEditor.tooltip.mode.watch",
  interval: "roomEditor.tooltip.mode.interval",
};

type RoomEditorModalProps = {
  open: boolean;
  title: string;
  submitLabel: string;
  formId: string;
  draft: RoomFormDraft;
  availableIccProfiles: AvailableIccProfileDto[];
  fieldErrors: RoomFormFieldErrors;
  editorError: string | null;
  submitting: boolean;
  pickingTargetPath: boolean;
  pickingIccProfile: boolean;
  onClose: () => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onPickTargetPath: () => void;
  onPickIccProfile: () => void;
  onDraftChange: (updater: (current: RoomFormDraft) => RoomFormDraft) => void;
};

export function RoomEditorModal({
  open,
  title,
  submitLabel,
  formId,
  draft,
  availableIccProfiles,
  fieldErrors,
  editorError,
  submitting,
  pickingTargetPath,
  pickingIccProfile,
  onClose,
  onSubmit,
  onPickTargetPath,
  onPickIccProfile,
  onDraftChange,
}: RoomEditorModalProps) {
  const { t, translateMaybe } = useI18n();
  const showIntervalField = draft.mode === "interval";
  const showWatchFields = draft.mode === "watch";
  const showCustomResolutionFields = draft.resolution_preset === "custom";
  const showIccFilePicker = draft.icc_profile_source === "file";
  const modeOptions = MODE_OPTIONS.map((option) => ({
    value: option.value,
    label: t(option.labelKey),
  }));
  const resolutionOptions = RESOLUTION_OPTIONS.map((option) => ({
    value: option.value,
    label: t(option.labelKey),
  }));
  const iccProfileOptions = [
    ...availableIccProfiles.map((profile) => ({
      value: `system:${profile.id}`,
      label: profile.display_name,
      supportingText: profile.is_primary
        ? t("roomEditor.primaryDisplayIcc")
        : undefined,
    })),
    {
      value: "file",
      label: t("roomEditor.iccProfileFromFile"),
      supportingText: t("roomEditor.clickToImportIccProfile"),
    },
  ];
  const selectedIccProfileOption =
    draft.icc_profile_source === "file"
      ? "file"
      : draft.icc_profile_system_id
        ? `system:${draft.icc_profile_system_id}`
        : undefined;

  return (
    <Modal
      open={open}
      size="lg"
      onClose={onClose}
      header={
        <ModalHeader
          title={title}
          closeButtonLabel={t("common.closeModal")}
          onClose={onClose}
        />
      }
      body={
        <ModalBody className="room-editor-modal__body">
          <form
            id={formId}
            className="room-editor-modal__form"
            onSubmit={onSubmit}
          >
            <div className="room-editor-modal__grid">
              <Input
                label={t("roomEditor.roomName")}
                title={t("roomEditor.tooltip.roomName")}
                className="room-editor-modal__field room-editor-modal__field--wide"
                error={
                  fieldErrors.name
                    ? translateMaybe(fieldErrors.name)
                    : undefined
                }
                value={draft.name}
                onChange={(event) =>
                  onDraftChange((current) => ({
                    ...current,
                    name: event.target.value,
                  }))
                }
              />
              <div className="room-editor-modal__field room-editor-modal__field--wide room-editor-modal__file-field">
                <span
                  className="room-editor-modal__label"
                  title={t("roomEditor.tooltip.targetPath")}
                >
                  {t("roomEditor.targetPath")}
                </span>
                <Button
                  type="button"
                  variant="secondary"
                  width="fill"
                  className="room-editor-modal__file-trigger"
                  title={t("roomEditor.tooltip.targetPath")}
                  disabled={submitting || pickingTargetPath}
                  onClick={onPickTargetPath}
                >
                  <span className="room-editor-modal__file-trigger-copy">
                    <span
                      className={cx(
                        "room-editor-modal__file-trigger-value",
                        !draft.target_path &&
                          "room-editor-modal__file-trigger-value--placeholder",
                      )}
                    >
                      {draft.target_path || t("roomEditor.chooseFile")}
                    </span>
                    <span className="room-editor-modal__file-trigger-hint">
                      {pickingTargetPath
                        ? t("roomEditor.openingFilePicker")
                        : t("roomEditor.clickToBrowse")}
                    </span>
                  </span>
                  <span className="room-editor-modal__file-trigger-icon">
                    <Icon name="folder" size="sm" aria-hidden />
                  </span>
                </Button>
              </div>
              <Select
                className="room-editor-modal__field room-editor-modal__field--wide"
                label={t("roomEditor.iccProfile")}
                title={t("roomEditor.tooltip.iccProfile")}
                options={iccProfileOptions}
                value={selectedIccProfileOption}
                onValueChange={(value) =>
                  onDraftChange((current) =>
                    value === "file"
                      ? {
                          ...current,
                          icc_profile_source: "file",
                          icc_profile_system_id: null,
                        }
                      : {
                          ...current,
                          icc_profile_source: "system",
                          icc_profile_system_id: value.slice("system:".length),
                        },
                  )
                }
              />
              <CheckboxRow
                className="room-editor-modal__field room-editor-modal__field--wide"
                label={t("roomEditor.iccProfileEnabled")}
                title={t("roomEditor.tooltip.iccProfileEnabled")}
                checked={draft.icc_profile_enabled}
                onCheckedChange={(checked) =>
                  onDraftChange((current) => ({
                    ...current,
                    icc_profile_enabled: checked,
                  }))
                }
              />
              {showIccFilePicker ? (
                <div className="room-editor-modal__field room-editor-modal__field--wide room-editor-modal__file-field">
                  <span
                    className="room-editor-modal__label"
                    title={t("roomEditor.tooltip.iccFile")}
                  >
                    {t("roomEditor.iccFile")}
                  </span>
                  <Button
                    type="button"
                    variant="secondary"
                    width="fill"
                    className="room-editor-modal__file-trigger"
                    title={t("roomEditor.tooltip.iccFile")}
                    disabled={submitting || pickingIccProfile}
                    onClick={onPickIccProfile}
                  >
                    <span className="room-editor-modal__file-trigger-copy">
                      <span
                        className={cx(
                          "room-editor-modal__file-trigger-value",
                          !draft.icc_profile_name &&
                            "room-editor-modal__file-trigger-value--placeholder",
                        )}
                      >
                        {draft.icc_profile_name || t("roomEditor.noIccProfile")}
                      </span>
                      <span className="room-editor-modal__file-trigger-hint">
                        {pickingIccProfile
                          ? t("roomEditor.importingIccProfile")
                          : t("roomEditor.clickToImportIccProfile")}
                      </span>
                    </span>
                    <span className="room-editor-modal__file-trigger-icon">
                      <Icon name="folder" size="sm" aria-hidden />
                    </span>
                  </Button>
                </div>
              ) : null}
              {fieldErrors.icc_profile ? (
                <p className="room-editor-modal__field-error room-editor-modal__field room-editor-modal__field--wide">
                  {translateMaybe(fieldErrors.icc_profile)}
                </p>
              ) : null}
              <div className="room-editor-modal__field room-editor-modal__field--wide room-editor-modal__mode-field">
                <span
                  className="room-editor-modal__label"
                  title={t("roomEditor.tooltip.mode")}
                >
                  {t("roomEditor.detectionMode")}
                </span>
                <div
                  className="room-editor-modal__mode-segmented"
                  role="tablist"
                  aria-label={t("roomEditor.detectionModeAria")}
                  title={t("roomEditor.tooltip.mode")}
                >
                  {modeOptions.map((option) => {
                    const isActive = draft.mode === option.value;

                    return (
                      <Button
                        key={option.value}
                        type="button"
                        width="fill"
                        variant={isActive ? "primary" : "ghost"}
                        className={cx(
                          "room-editor-modal__mode-button",
                          isActive && "room-editor-modal__mode-button--active",
                        )}
                        title={t(
                          MODE_TOOLTIP_KEYS[option.value as DetectionMode],
                        )}
                        onClick={() =>
                          onDraftChange((current) => ({
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
                  label={t("roomEditor.intervalMs")}
                  title={t("roomEditor.tooltip.intervalMs")}
                  type="number"
                  min={1}
                  error={
                    fieldErrors.interval_ms
                      ? translateMaybe(fieldErrors.interval_ms)
                      : undefined
                  }
                  value={draft.interval_ms}
                  onChange={(event) =>
                    onDraftChange((current) => ({
                      ...current,
                      interval_ms: event.target.value,
                    }))
                  }
                />
              ) : null}
              {showWatchFields ? (
                <Input
                  label={t("roomEditor.debounceMs")}
                  title={t("roomEditor.tooltip.debounceMs")}
                  type="number"
                  min={0}
                  error={
                    fieldErrors.debounce_ms
                      ? translateMaybe(fieldErrors.debounce_ms)
                      : undefined
                  }
                  value={draft.debounce_ms}
                  onChange={(event) =>
                    onDraftChange((current) => ({
                      ...current,
                      debounce_ms: event.target.value,
                    }))
                  }
                />
              ) : null}
              {showWatchFields ? (
                <Input
                  label={t("roomEditor.stabilizeMs")}
                  title={t("roomEditor.tooltip.stabilizeMs")}
                  type="number"
                  min={0}
                  error={
                    fieldErrors.stabilize_ms
                      ? translateMaybe(fieldErrors.stabilize_ms)
                      : undefined
                  }
                  value={draft.stabilize_ms}
                  onChange={(event) =>
                    onDraftChange((current) => ({
                      ...current,
                      stabilize_ms: event.target.value,
                    }))
                  }
                />
              ) : null}
              <Select
                className="room-editor-modal__field room-editor-modal__field--wide"
                label={t("roomEditor.resolution")}
                title={t("roomEditor.tooltip.resolution")}
                options={resolutionOptions}
                value={draft.resolution_preset}
                onValueChange={(value) =>
                  onDraftChange((current) => {
                    const nextPreset =
                      value as RoomFormDraft["resolution_preset"];

                    if (
                      nextPreset !== "source" &&
                      nextPreset !== "custom" &&
                      nextPreset in RESOLUTION_PRESETS
                    ) {
                      const preset =
                        RESOLUTION_PRESETS[
                          nextPreset as Exclude<
                            ResolutionPreset,
                            "source" | "custom"
                          >
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
                    label={t("roomEditor.maxWidth")}
                    title={t("roomEditor.tooltip.maxWidth")}
                    type="number"
                    min={1}
                    error={
                      fieldErrors.max_width
                        ? translateMaybe(fieldErrors.max_width)
                        : undefined
                    }
                    value={draft.max_width}
                    onChange={(event) =>
                      onDraftChange((current) => ({
                        ...current,
                        max_width: event.target.value,
                      }))
                    }
                  />
                  <Input
                    label={t("roomEditor.maxHeight")}
                    title={t("roomEditor.tooltip.maxHeight")}
                    type="number"
                    min={1}
                    error={
                      fieldErrors.max_height
                        ? translateMaybe(fieldErrors.max_height)
                        : undefined
                    }
                    value={draft.max_height}
                    onChange={(event) =>
                      onDraftChange((current) => ({
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
              label={t("roomEditor.detectionEnabled")}
              title={t("roomEditor.tooltip.detectionEnabled")}
              checked={draft.detection_enabled}
              onCheckedChange={(checked) =>
                onDraftChange((current) => ({
                  ...current,
                  detection_enabled: checked,
                }))
              }
            />
          </form>
          {editorError ? (
            <p className="room-editor-modal__error">
              {translateMaybe(editorError)}
            </p>
          ) : null}
        </ModalBody>
      }
      footer={
        <ModalFooter>
          <Button
            variant="ghost"
            onClick={onClose}
            disabled={submitting || pickingTargetPath || pickingIccProfile}
          >
            {t("common.cancel")}
          </Button>
          <Button
            form={formId}
            type="submit"
            disabled={submitting || pickingTargetPath || pickingIccProfile}
          >
            {submitting ? t("common.saving") : submitLabel}
          </Button>
        </ModalFooter>
      }
    />
  );
}
