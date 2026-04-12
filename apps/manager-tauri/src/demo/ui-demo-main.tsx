import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../app/index.css';
import { I18nProvider, detectPreferredLocale } from '../shared/i18n';
import { translateForLocale } from '../shared/i18n/resources';
import { DemoShell } from './DemoShell';
import { UiDemoPage } from './UiDemoPage';

const rootElement = document.getElementById('root');

if (!rootElement) {
  throw new Error(
    translateForLocale(detectPreferredLocale(), 'app.error.rootElementNotFound'),
  );
}

createRoot(rootElement).render(
  <StrictMode>
    <I18nProvider>
      <DemoShell>
        <UiDemoPage />
      </DemoShell>
    </I18nProvider>
  </StrictMode>,
);
