import { create } from 'zustand';

type TranscriptSegment = {
  segmentId: string;
  channel: string;
  speaker: string | null;
  text: string;
  isPartial: boolean;
  startTime: number;
  endTime: number;
};

type TranscriptState = {
  segments: TranscriptSegment[];
  upsert: (segment: TranscriptSegment) => void;
  remove: (segmentId: string) => void;
  clear: () => void;
};

export const useTranscriptStore = create<TranscriptState>((set) => ({
  segments: [],
  upsert: (segment) =>
    set((state) => {
      const idx = state.segments.findIndex((s) => s.segmentId === segment.segmentId);
      if (idx >= 0) {
        const next = state.segments.slice();
        next[idx] = segment;
        return { segments: next };
      }
      return { segments: [...state.segments, segment] };
    }),
  remove: (segmentId) =>
    set((state) => ({ segments: state.segments.filter((s) => s.segmentId !== segmentId) })),
  clear: () => set({ segments: [] }),
}));

export type { TranscriptSegment };
