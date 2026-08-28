import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import App from '@/App';
import { AppProviders } from '@/AppProviders';

vi.mock('@/lib/tauri', () => ({
  capturePermissions: vi.fn().mockResolvedValue({
    screenRecording: 'Granted',
    microphone: 'Granted',
  }),
  captureStatus: vi.fn().mockResolvedValue({
    phase: 'Idle',
    meetingId: null,
    errorCode: null,
  }),
  deleteMeeting: vi.fn().mockResolvedValue(undefined),
  getProviderSettings: vi.fn().mockResolvedValue({
    provider: 'deepgram',
    model: 'nova-3',
    language: 'en',
    azureRegion: null,
    hasSecret: true,
    diarizeSystem: false,
    diarizeMic: true,
  }),
  listMeetingSegments: vi.fn().mockResolvedValue([]),
  listMeetings: vi.fn().mockResolvedValue([]),
  onCaptureStatus: vi.fn().mockResolvedValue(() => undefined),
  onMeetingEvent: vi.fn().mockResolvedValue(() => undefined),
  openCapturePermissionSettings: vi.fn().mockResolvedValue(undefined),
  requestCapturePermission: vi.fn().mockResolvedValue(undefined),
  pauseMeeting: vi.fn().mockResolvedValue(undefined),
  resumeMeeting: vi.fn().mockResolvedValue(undefined),
  saveProviderSettings: vi.fn().mockResolvedValue(undefined),
  startMeeting: vi.fn().mockResolvedValue(undefined),
  stopMeeting: vi.fn().mockResolvedValue(undefined),
}));

describe('AppRoot', () => {
  it('renders routes with application providers available to shell hooks', async () => {
    render(
      <AppProviders>
        <MemoryRouter>
          <App />
        </MemoryRouter>
      </AppProviders>,
    );

    expect(await screen.findByRole('heading', { name: 'Welcome back' })).toBeVisible();
    expect(screen.getByRole('navigation')).toBeVisible();
  });
});
