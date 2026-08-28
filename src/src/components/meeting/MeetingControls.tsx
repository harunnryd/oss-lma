import { Mic, MicOff, Pause, Play, Square } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { useMeetingStore } from '@/stores/meetingStore';
import { pauseMeeting, resumeMeeting, startMeeting, stopMeeting } from '@/lib/tauri';
import { useToast } from '@/components/ui/toast-context';

export function MeetingControls() {
  const phase = useMeetingStore((s) => s.phase);
  const { push } = useToast();

  async function withToast(label: string, fn: () => Promise<void>) {
    try {
      await fn();
    } catch (error) {
      push({ title: `${label} failed`, description: String(error), variant: 'destructive' });
    }
  }

  const isActive = phase === 'Active';
  const isPaused = phase === 'Paused';
  const canStart = phase === 'Idle' || phase === 'Failed';
  const canStop = isActive || isPaused;

  return (
    <div className="flex flex-wrap gap-2">
      <Button onClick={() => withToast('Start', startMeeting)} disabled={!canStart} className="min-w-32">
        <Mic className="h-4 w-4" /> Start meeting
      </Button>
      <Button
        variant="secondary"
        onClick={() => withToast('Pause', pauseMeeting)}
        disabled={!isActive}
      >
        <Pause className="h-4 w-4" /> Pause
      </Button>
      <Button
        variant="secondary"
        onClick={() => withToast('Resume', resumeMeeting)}
        disabled={!isPaused}
      >
        <Play className="h-4 w-4" /> Resume
      </Button>
      <Button variant="destructive" onClick={() => withToast('Stop', stopMeeting)} disabled={!canStop}>
        <Square className="h-4 w-4" /> Stop
      </Button>
      {phase === 'Failed' && (
        <span className="ml-2 inline-flex items-center gap-1 text-sm text-destructive">
          <MicOff className="h-4 w-4" /> Last attempt failed
        </span>
      )}
    </div>
  );
}
