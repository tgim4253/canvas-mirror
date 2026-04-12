import { useEffect } from 'react';
import { HashRouter, Navigate, Route, Routes } from 'react-router-dom';
import { StudioPage } from '../pages/studio';
import { useI18n } from '../shared/i18n';
import { useTheme } from '../shared/hooks';

export default function App() {
  const { locale, t } = useI18n();
  useTheme();

  useEffect(() => {
    document.title = t('app.title');
  }, [locale, t]);

  return (
    <HashRouter>
      <Routes>
        <Route path="/" element={<StudioPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </HashRouter>
  );
}
