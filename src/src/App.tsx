import { Navigate, Route, Routes } from 'react-router-dom';

import { AppShell } from '@/components/layout/AppShell';
import { DashboardPage } from '@/pages/Dashboard';
import { OnboardingPage } from '@/pages/Onboarding';
import { LivePage } from '@/pages/Live';
import { SettingsPage } from '@/pages/Settings';
import { HistoryPage } from '@/pages/History';
import { HistoryDetailPage } from '@/pages/HistoryDetail';

export default function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<DashboardPage />} />
        <Route path="onboarding" element={<OnboardingPage />} />
        <Route path="live" element={<LivePage />} />
        <Route path="settings" element={<SettingsPage />} />
        <Route path="history" element={<HistoryPage />} />
        <Route path="history/:meetingId" element={<HistoryDetailPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
