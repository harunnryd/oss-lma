import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { MeetingControls } from '@/components/meeting/MeetingControls';
import { RecoveryBanner } from '@/components/meeting/RecoveryBanner';
import { TranscriptView } from '@/components/meeting/TranscriptView';
import { useMeetingStore } from '@/stores/meetingStore';

export function LivePage() {
  const phase = useMeetingStore((s) => s.phase);
  const meetingId = useMeetingStore((s) => s.meetingId);

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight">Live meeting</h1>
        <p className="text-sm text-muted-foreground">
          Capture, pause, and resume meetings. Transcript streams here as the sidecar emits frames.
        </p>
      </header>

      <RecoveryBanner />

      <Card>
        <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
          <div>
            <CardTitle className="text-base">Controls</CardTitle>
            <CardDescription>
              {meetingId
                ? `Current meeting ${meetingId.slice(0, 8)} — phase ${phase}.`
                : 'No active meeting. Start one to begin capturing.'}
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent>
          <MeetingControls />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Transcript</CardTitle>
          <CardDescription>Partial segments update in place. Final segments stay solid.</CardDescription>
        </CardHeader>
        <CardContent>
          <TranscriptView />
        </CardContent>
      </Card>
    </div>
  );
}
