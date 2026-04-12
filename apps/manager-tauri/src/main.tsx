import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/inter/700.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import '@fontsource/manrope/600.css';
import '@fontsource/manrope/700.css';
import './app/index.css';
import App from './app';
import { I18nProvider, detectPreferredLocale } from './shared/i18n';
import { translateForLocale } from './shared/i18n/resources';

const rootElement = document.getElementById('root');

if (!rootElement) {
  throw new Error(
    translateForLocale(detectPreferredLocale(), 'app.error.rootElementNotFound'),
  );
}

createRoot(rootElement).render(
  <StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </StrictMode>,
);
