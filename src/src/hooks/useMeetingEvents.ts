import { useEffect } from 'react';

import { onCaptureStatus, onMeetingEvent, type WireEvent } from '@/lib/tauri';
import { useMeetingStore } from '@/stores/meetingStore';
import { usePermissionStore } from '@/stores/permissionStore';
import { useTranscriptStore } from '@/stores/transcriptStore';
import { useToast } from '@/components/ui/toast-context';

export function useMeetingEvents() {
  const setStatus = useMeetingStore((s) => s.setStatus);
  const upsert = useTranscriptStore((s) => s.upsert);
  const remove = useTranscriptStore((s) => s.remove);
  const clearTranscript = useTranscriptStore((s) => s.clear);
  const { push } = useToast();

  useEffect(() => {
    const unlistenStatus = onCaptureStatus((status) => {
      setStatus({ phase: status.phase, meetingId: status.meetingId, errorCode: status.errorCode });
    });

    const unlistenEvent = onMeetingEvent((event: WireEvent) => {
      switch (event.EventType) {
        case 'START':
          clearTranscript();
          break;
        case 'ADD_TRANSCRIPT_SEGMENT':
          upsert({
            segmentId: event.SegmentId,
            channel: event.Channel,
            speaker: event.Speaker ?? null,
            text: event.Transcript,
            isPartial: event.IsPartial,
            startTime: event.StartTime,
            endTime: event.EndTime,
          });
          break;
        case 'DELETE_TRANSCRIPT_SEGMENT':
          remove(event.SegmentId);
          break;
        case 'ERROR':
          push({
            title: 'Sidecar reported an error',
            description: `${event.Code}${event.CallId ? ` · call ${event.CallId.slice(0, 8)}` : ''}`,
            variant: 'destructive',
          });
          break;
      }
    });

    return () => {
      unlistenStatus.then((f) => f());
      unlistenEvent.then((f) => f());
    };
  }, [setStatus, upsert, remove, clearTranscript, push]);
}

export function usePermissionsSync() {
  const setPermissions = usePermissionStore((s) => s.setPermissions);
  const perms = usePermissionStore((s) => s.permissions);
  useEffect(() => {
    setPermissions(perms);
  }, [perms, setPermissions]);
}
