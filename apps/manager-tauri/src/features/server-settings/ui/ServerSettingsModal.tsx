import type { FormEvent } from "react";
import { useI18n, type Locale } from "../../../shared/i18n";
import {
  Input,
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader,
  Button,
  Icon,
  Select,
} from "../../../shared/ui";
import {
  bindHostForPreset,
  bindHostPresetFromHost,
  describeCustomBindHost,
  describeServerExposureWarning,
  LOOPBACK_BIND_HOST,
  type ServerSettingsFieldErrors,
  type ServerSettingsDraft,
  WILDCARD_BIND_HOST,
} from "../model/serverSettings";
import "./server-settings-modal.css";

const BIND_HOST_OPTIONS = [
  {
    value: "loopback",
    labelKey: "serverSettings.bindHostPreset.localOnly",
    supportingText: LOOPBACK_BIND_HOST,
  },
  {
    value: "wildcard",
    labelKey: "serverSettings.bindHostPreset.localNetwork",
    supportingText: WILDCARD_BIND_HOST,
  },
];

const LANGUAGE_OPTIONS: Array<{
  value: Locale;
  labelKey: string;
}> = [
  {
    value: "en",
    labelKey: "serverSettings.languageOption.en",
  },
  {
    value: "ko",
    labelKey: "serverSettings.languageOption.ko",
  },
  {
    value: "jp",
    labelKey: "serverSettings.languageOption.jp",
  },
];

type ServerSettingsModalProps = {
  open: boolean;
  formId: string;
  draft: ServerSettingsDraft;
  fieldErrors: ServerSettingsFieldErrors;
  loading: boolean;
  submitting: boolean;
  error: string | null;
  onClose: () => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onDraftChange: (
    updater: (current: ServerSettingsDraft) => ServerSettingsDraft,
  ) => void;
};

export function ServerSettingsModal({
  open,
  formId,
  draft,
  fieldErrors,
  loading,
  submitting,
  error,
  onClose,
  onSubmit,
  onDraftChange,
}: ServerSettingsModalProps) {
  const { locale, setLocale, t, translateMaybe } = useI18n();
  const disabled = loading || submitting;
  const exposureWarning = describeServerExposureWarning(draft);
  const bindHostPreset = bindHostPresetFromHost(draft.bind_host);
  const customBindHost = describeCustomBindHost(draft.bind_host);
  const languageOptions = LANGUAGE_OPTIONS.map((option) => ({
    value: option.value,
    label: t(option.labelKey),
  }));
  const bindHostOptions = BIND_HOST_OPTIONS.map((option) => ({
    value: option.value,
    label: t(option.labelKey),
    supportingText: option.supportingText,
  }));

  return (
    <Modal
      open={open}
      size="md"
      onClose={disabled ? undefined : onClose}
      header={
        <ModalHeader
          title={t("serverSettings.title")}
          closeButtonLabel={t("common.closeModal")}
          onClose={disabled ? undefined : onClose}
        />
      }
      body={
        <ModalBody className="server-settings-modal__body">
          <form
            id={formId}
            className="server-settings-modal__form"
            onSubmit={onSubmit}
          >
            {exposureWarning ? (
              <div className="server-settings-modal__warning" role="alert">
                <Icon name="warning" size="sm" aria-hidden />
                <p>{translateMaybe(exposureWarning)}</p>
              </div>
            ) : null}
            <div className="server-settings-modal__grid">
              <div className="server-settings-modal__field server-settings-modal__field--wide">
                <Select
                  label={t("serverSettings.language")}
                  title={t("serverSettings.tooltip.language")}
                  options={languageOptions}
                  value={locale}
                  disabled={submitting}
                  onValueChange={(value) => setLocale(value)}
                />
              </div>
              <div className="server-settings-modal__field">
                <Select
                  label={t("serverSettings.bindHost")}
                  title={t("serverSettings.tooltip.bindHost")}
                  options={bindHostOptions}
                  value={bindHostPreset}
                  disabled={disabled}
                  onValueChange={(value) =>
                    onDraftChange((current) => ({
                      ...current,
                      bind_host: bindHostForPreset(
                        value === "loopback" ? "loopback" : "wildcard",
                      ),
                    }))
                  }
                />
                {fieldErrors.bind_host ? (
                  <p className="server-settings-modal__field-note server-settings-modal__field-note--error">
                    {translateMaybe(fieldErrors.bind_host)}
                  </p>
                ) : null}
                <p className="server-settings-modal__field-note">
                  {t("serverSettings.bindHostNote.localNetwork", {
                    host: WILDCARD_BIND_HOST,
                  })}
                </p>
                {customBindHost ? (
                  <p className="server-settings-modal__field-note">
                    {t("serverSettings.bindHostNote.currentConfig", {
                      host: customBindHost,
                    })}
                  </p>
                ) : null}
              </div>
              <Input
                label={t("serverSettings.port")}
                title={t("serverSettings.tooltip.port")}
                type="number"
                min={1}
                max={65535}
                error={
                  fieldErrors.bind_port
                    ? translateMaybe(fieldErrors.bind_port)
                    : undefined
                }
                value={draft.bind_port}
                disabled={disabled}
                onChange={(event) =>
                  onDraftChange((current) => ({
                    ...current,
                    bind_port: event.target.value,
                  }))
                }
              />
              <Input
                label={t("serverSettings.publicUrl")}
                title={t("serverSettings.tooltip.publicUrl")}
                className="server-settings-modal__field--wide"
                error={
                  fieldErrors.public_url
                    ? translateMaybe(fieldErrors.public_url)
                    : undefined
                }
                hint={t("serverSettings.publicUrlHint")}
                value={draft.public_url}
                disabled={disabled}
                onChange={(event) =>
                  onDraftChange((current) => ({
                    ...current,
                    public_url: event.target.value,
                  }))
                }
              />
              <Input
                label={t("serverSettings.staleTimeout")}
                title={t("serverSettings.tooltip.staleTimeout")}
                className="server-settings-modal__field--wide"
                type="number"
                min={0}
                error={
                  fieldErrors.stale_timeout_ms
                    ? translateMaybe(fieldErrors.stale_timeout_ms)
                    : undefined
                }
                value={draft.stale_timeout_ms}
                disabled={disabled}
                onChange={(event) =>
                  onDraftChange((current) => ({
                    ...current,
                    stale_timeout_ms: event.target.value,
                  }))
                }
              />
            </div>
          </form>
          {error ? (
            <p className="server-settings-modal__error">
              {translateMaybe(error)}
            </p>
          ) : null}
        </ModalBody>
      }
      footer={
        <ModalFooter>
          <Button variant="ghost" onClick={onClose} disabled={disabled}>
            {t("common.cancel")}
          </Button>
          <Button form={formId} type="submit" disabled={disabled}>
            {submitting ? t("common.saving") : t("serverSettings.save")}
          </Button>
        </ModalFooter>
      }
    />
  );
}
