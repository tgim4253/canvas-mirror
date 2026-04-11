import { HashRouter, Navigate, Route, Routes } from 'react-router-dom';
import { StudioPage } from '../pages/studio';
import { useTheme } from '../shared/hooks';

export default function App() {
  useTheme();

  return (
    <HashRouter>
      <Routes>
        <Route path="/" element={<StudioPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </HashRouter>
  );
}
