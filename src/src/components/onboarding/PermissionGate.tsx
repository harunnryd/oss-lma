import { useEffect } from 'react';
import { ShieldAlert, ShieldCheck, ShieldQuestion } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { usePermissions } from '@/hooks/useTauriCommand';
import { openCapturePermissionSettings } from '@/lib/tauri';
import { useToast } from '@/components/ui/toaster';
import { cn } from '@/lib/utils';

export function PermissionGate() {
  const { data, refetch } = usePermissions();
  const { push } = useToast();

  // Re-query after returning from the system settings pane.
  useEffect(() => {
    function onFocus() {
      refetch();
    }
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [refetch]);

  async function open(kind: 'microphone' | 'screenRecording') {
    try {
      await openCapturePermissionSettings(kind);
      push({ title: 'Opening System Settings…', description: 'Grant access, then return to the app.' });
    } catch (error) {
      push({ title: 'Could not open settings', description: String(error), variant: 'destructive' });
    }
  }

  if (!data) {
    return (
      <Card>
        <CardContent className="py-8 text-center text-sm text-muted-foreground">
          Checking system permissions…
        </CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Capture access</CardTitle>
        <CardDescription>
          oss-lma needs microphone and screen-recording access to capture your meetings. Grant both, then return here.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <PermissionRow
          label="Microphone"
          status={data.microphone}
          onClick={() => open('microphone')}
        />
        <PermissionRow
          label="Screen recording"
          status={data.screenRecording}
          onClick={() => open('screenRecording')}
        />
      </CardContent>
    </Card>
  );
}

function PermissionRow({
  label,
  status,
  onClick,
}: {
  label: string;
  status: 'Unknown' | 'Denied' | 'Granted';
  onClick: () => void;
}) {
  const Icon = status === 'Granted' ? ShieldCheck : status === 'Denied' ? ShieldAlert : ShieldQuestion;
  const color =
    status === 'Granted' ? 'text-emerald-400' : status === 'Denied' ? 'text-destructive' : 'text-muted-foreground';
  return (
    <div className="flex items-center justify-between rounded-md border border-border bg-card/60 px-4 py-3">
      <div className="flex items-center gap-3">
        <Icon className={cn('h-4 w-4', color)} />
        <div className="flex flex-col leading-tight">
          <span className="text-sm font-medium">{label}</span>
          <span className={cn('text-xs', color)}>
            {status === 'Granted' ? 'Granted' : status === 'Denied' ? 'Denied — click to grant' : 'Unknown'}
          </span>
        </div>
      </div>
      <Button variant="secondary" size="sm" onClick={onClick}>
        Open settings
      </Button>
    </div>
  );
}
