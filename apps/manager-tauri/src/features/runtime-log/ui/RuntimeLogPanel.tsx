import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../../../shared/i18n';
import { cx } from '../../../shared/lib/cx';
import { toDocumentLang } from '../../../shared/i18n/resources';
import type { LogEntryDto } from '../../../shared/type';
import { Icon } from '../../../shared/ui';
import './runtime-log-panel.css';

type RuntimeLogPanelProps = {
  logs: LogEntryDto[];
  runtimeError: string | null;
  runtimeGeneratedAt: string | null;
};

const MAX_VISIBLE_LOGS = 160;
const LOG_SHEET_FADE_OUT_MS = 140;

function formatLogTimestamp(value: string, locale: string) {
  const parsed = new Date(value);

  if (Number.isNaN(parsed.getTime())) {
    return '--:--:--';
  }

  return parsed.toLocaleTimeString(locale, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
}

function formatLogScope(scope: string) {
  return scope.replace(/[:/]/g, ' ').toUpperCase();
}

export function RuntimeLogPanel({
  logs,
  runtimeError,
  runtimeGeneratedAt,
}: RuntimeLogPanelProps) {
  const { locale, t, translateMaybe } = useI18n();
  const intlLocale = toDocumentLang(locale);
  const [logPanelExpanded, setLogPanelExpanded] = useState(false);
  const [logPanelRendered, setLogPanelRendered] = useState(false);
  const [logPanelClosing, setLogPanelClosing] = useState(false);
  const logPanelRootRef = useRef<HTMLDivElement | null>(null);
  const logViewportRef = useRef<HTMLDivElement | null>(null);
  const logSheetFadeTimeoutRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (logSheetFadeTimeoutRef.current !== null) {
        window.clearTimeout(logSheetFadeTimeoutRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!logPanelExpanded) {
      return;
    }

    const viewport = logViewportRef.current;
    if (!viewport) {
      return;
    }

    viewport.scrollTop = 0;
  }, [logs.length, logPanelExpanded, logPanelRendered, logPanelClosing]);

  useEffect(() => {
    if (!logPanelExpanded || !logPanelRendered || logPanelClosing) {
      return;
    }

    const handlePointerDown = (event: PointerEvent) => {
      const panelRoot = logPanelRootRef.current;
      const target = event.target;

      if (!panelRoot || !(target instanceof Node) || panelRoot.contains(target)) {
        return;
      }

      closeLogPanel();
    };

    document.addEventListener('pointerdown', handlePointerDown);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown);
    };
  }, [logPanelClosing, logPanelExpanded, logPanelRendered]);

  const openLogPanel = () => {
    if (logSheetFadeTimeoutRef.current !== null) {
      window.clearTimeout(logSheetFadeTimeoutRef.current);
      logSheetFadeTimeoutRef.current = null;
    }

    setLogPanelClosing(false);
    setLogPanelRendered(true);
    setLogPanelExpanded(true);
  };

  const closeLogPanel = () => {
    if (!logPanelRendered) {
      return;
    }

    if (logSheetFadeTimeoutRef.current !== null) {
      window.clearTimeout(logSheetFadeTimeoutRef.current);
    }

    setLogPanelExpanded(false);
    setLogPanelClosing(true);

    logSheetFadeTimeoutRef.current = window.setTimeout(() => {
      setLogPanelRendered(false);
      setLogPanelClosing(false);
      logSheetFadeTimeoutRef.current = null;
    }, LOG_SHEET_FADE_OUT_MS);
  };

  const toggleLogPanel = () => {
    if (logPanelExpanded && !logPanelClosing) {
      closeLogPanel();
      return;
    }

    openLogPanel();
  };

  const visibleLogs = logs.slice(-MAX_VISIBLE_LOGS).reverse();
  const runtimeUpdatedLabel = runtimeGeneratedAt
    ? t('runtime.updated', { time: formatLogTimestamp(runtimeGeneratedAt, intlLocale) })
    : t('runtime.waiting');
  const logPanelMeta = runtimeError
    ? t('runtime.connectionError')
    : t('runtime.meta', { count: visibleLogs.length, updatedLabel: runtimeUpdatedLabel });

  return (
    <div
      ref={logPanelRootRef}
      className={cx(
        'runtime-log-panel',
        runtimeError && 'runtime-log-panel--error',
      )}
    >
      {logPanelRendered ? (
        <section
          id="studio-runtime-log-sheet"
          className={cx(
            'runtime-log-panel__sheet',
            logPanelClosing && 'runtime-log-panel__sheet--closing',
          )}
          aria-label={t('runtime.logsAria')}
          aria-hidden={logPanelClosing}
        >
          <div ref={logViewportRef} className="runtime-log-panel__viewport" aria-live="polite">
            {runtimeError ? (
              <p className="runtime-log-panel__empty runtime-log-panel__empty--error">
                {translateMaybe(runtimeError)}
              </p>
            ) : visibleLogs.length === 0 ? (
              <p className="runtime-log-panel__empty">{t('runtime.emptyLive')}</p>
            ) : (
              <ol className="runtime-log-panel__list">
                {visibleLogs.map((log, index) => (
                  <li
                    key={`${log.at}-${log.scope}-${index}`}
                    className={cx(
                      'runtime-log-panel__line',
                      `runtime-log-panel__line--${log.level}`,
                    )}
                  >
                    <span className="runtime-log-panel__time">
                      [{formatLogTimestamp(log.at, intlLocale)}]
                    </span>
                    <span className="runtime-log-panel__scope">{formatLogScope(log.scope)}</span>
                    <span className="runtime-log-panel__message">{log.message}</span>
                  </li>
                ))}
              </ol>
            )}
          </div>
        </section>
      ) : null}
      <button
        type="button"
        className="runtime-log-panel__trigger"
        aria-expanded={logPanelExpanded}
        aria-controls="studio-runtime-log-sheet"
        onClick={toggleLogPanel}
      >
        <span className="runtime-log-panel__trigger-copy">
          <span className="runtime-log-panel__trigger-title">{t('runtime.logButton')}</span>
          <span className="runtime-log-panel__trigger-meta">{logPanelMeta}</span>
        </span>
        <Icon name={logPanelExpanded ? 'chevron-down' : 'chevron-up'} size="xs" aria-hidden />
      </button>
    </div>
  );
}
