import { create } from 'zustand';

import type { CapturePhase } from '@/lib/tauri';

type MeetingState = {
  phase: CapturePhase;
  meetingId: string | null;
  errorCode: string | null;
  setStatus: (status: { phase: CapturePhase; meetingId: string | null; errorCode: string | null }) => void;
  reset: () => void;
};

export const useMeetingStore = create<MeetingState>((set) => ({
  phase: 'Idle',
  meetingId: null,
  errorCode: null,
  setStatus: (status) => set(status),
  reset: () => set({ phase: 'Idle', meetingId: null, errorCode: null }),
}));
