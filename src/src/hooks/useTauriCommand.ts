import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';

import {
  capturePermissions,
  captureStatus,
  deleteMeeting,
  getProviderSettings,
  listMeetingSegments,
  listMeetings,
  saveProviderSettings,
} from '@/lib/tauri';
import type { ProviderDraft } from '@/lib/tauri';

export function useCaptureStatus() {
  return useQuery({ queryKey: ['captureStatus'], queryFn: captureStatus, refetchInterval: 1500 });
}

export function usePermissions() {
  return useQuery({ queryKey: ['permissions'], queryFn: capturePermissions, refetchInterval: 2000 });
}

export function useProviderSettings() {
  return useQuery({ queryKey: ['providerSettings'], queryFn: getProviderSettings });
}

export function useSaveProviderSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (draft: ProviderDraft) => saveProviderSettings(draft),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['providerSettings'] });
    },
  });
}

export function useMeetings(limit = 50) {
  return useQuery({ queryKey: ['meetings', limit], queryFn: () => listMeetings(limit) });
}

export function useMeetingSegments(meetingId: string | null) {
  return useQuery({
    queryKey: ['meetingSegments', meetingId],
    queryFn: () => (meetingId ? listMeetingSegments(meetingId) : Promise.resolve([])),
    enabled: !!meetingId,
  });
}

export function useDeleteMeeting() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (meetingId: string) => deleteMeeting(meetingId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['meetings'] });
    },
  });
}
