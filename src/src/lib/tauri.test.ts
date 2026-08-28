import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  capturePermissions,
  captureStatus,
  requestCapturePermission,
  saveProviderSettings,
  startMeeting,
} from '@/lib/tauri';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

describe('Tauri provider settings adapter', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('sends public settings and the provider secret as separate command arguments', async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    await saveProviderSettings({
      provider: 'deepgram',
      model: 'nova-3',
      language: 'en',
      azureRegion: null,
      apiKey: 'private-key',
      diarizeSystem: false,
      diarizeMic: true,
    });

    expect(invoke).toHaveBeenCalledWith('save_provider_settings', {
      settings: {
        provider: 'deepgram',
        model: 'nova-3',
        language: 'en',
        azureRegion: null,
        diarizeSystem: false,
        diarizeMic: true,
      },
      secret: 'private-key',
    });
  });
});

describe('Tauri capture adapters', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('normalizes permission enum values from the Rust wire format', async () => {
    vi.mocked(invoke).mockResolvedValue({
      screenRecording: 'unknown',
      microphone: 'granted',
    });

    await expect(capturePermissions()).resolves.toEqual({
      screenRecording: 'Unknown',
      microphone: 'Granted',
    });
  });

  it('normalizes a requested permission result from the Rust wire format', async () => {
    vi.mocked(invoke).mockResolvedValue('denied');

    await expect(requestCapturePermission('screenRecording')).resolves.toBe('Denied');
  });

  it('normalizes capture status fields and phase values', async () => {
    vi.mocked(invoke).mockResolvedValue({
      phase: 'failed',
      meetingId: 'meeting-1',
      recordingPath: '/tmp/meeting.wav',
      systemActive: false,
      microphoneActive: false,
      error: 'CAPTURE_DEVICE_LOST',
    });

    await expect(captureStatus()).resolves.toEqual({
      phase: 'Failed',
      meetingId: 'meeting-1',
      errorCode: 'CAPTURE_DEVICE_LOST',
    });
  });

  it('sends start meeting options using the command argument shape', async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    await startMeeting({ diarizeMicrophone: true });

    expect(invoke).toHaveBeenCalledWith('start_meeting', {
      options: { diarizeMicrophone: true },
    });
  });
});
