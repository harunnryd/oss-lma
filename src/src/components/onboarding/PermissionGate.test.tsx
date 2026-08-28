import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { PermissionGate } from '@/components/onboarding/PermissionGate';
import { ToasterProvider } from '@/components/ui/toaster';
import * as tauri from '@/lib/tauri';

vi.mock('@/lib/tauri', () => ({
  capturePermissions: vi.fn(),
  openCapturePermissionSettings: vi.fn().mockResolvedValue(undefined),
  requestCapturePermission: vi.fn().mockResolvedValue('Granted'),
}));

function renderGate() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <ToasterProvider>
        <PermissionGate />
      </ToasterProvider>
    </QueryClientProvider>,
  );
}

describe('PermissionGate', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('requests an unknown screen-recording permission before opening settings', async () => {
    vi.mocked(tauri.capturePermissions).mockResolvedValue({
      microphone: 'Granted',
      screenRecording: 'Unknown',
    });
    renderGate();

    fireEvent.click(
      await screen.findByRole('button', { name: 'Request access for Screen recording' }),
    );

    await waitFor(() =>
      expect(tauri.requestCapturePermission).toHaveBeenCalledWith('screenRecording'),
    );
    expect(tauri.openCapturePermissionSettings).not.toHaveBeenCalled();
  });

  it('opens settings for a permission that was already denied', async () => {
    vi.mocked(tauri.capturePermissions).mockResolvedValue({
      microphone: 'Granted',
      screenRecording: 'Denied',
    });
    renderGate();

    fireEvent.click(
      await screen.findByRole('button', { name: 'Open settings for Screen recording' }),
    );

    await waitFor(() =>
      expect(tauri.openCapturePermissionSettings).toHaveBeenCalledWith('screenRecording'),
    );
    expect(tauri.requestCapturePermission).not.toHaveBeenCalled();
  });

  it('opens settings when a native request does not grant access', async () => {
    vi.mocked(tauri.capturePermissions).mockResolvedValue({
      microphone: 'Unknown',
      screenRecording: 'Granted',
    });
    vi.mocked(tauri.requestCapturePermission).mockResolvedValue('Denied');
    renderGate();

    fireEvent.click(await screen.findByRole('button', { name: 'Request access for Microphone' }));

    await waitFor(() =>
      expect(tauri.openCapturePermissionSettings).toHaveBeenCalledWith('microphone'),
    );
  });
});
