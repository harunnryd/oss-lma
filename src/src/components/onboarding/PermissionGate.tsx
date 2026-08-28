import { useEffect } from 'react';
import { ShieldAlert, ShieldCheck, ShieldQuestion } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { usePermissions } from '@/hooks/useTauriCommand';
import { openCapturePermissionSettings, requestCapturePermission } from '@/lib/tauri';
import { useToast } from '@/components/ui/toast-context';
import { cn } from '@/lib/utils';

export function PermissionGate() {
  const { data, refetch } = usePermissions();
  const { push } = useToast();

  useEffect(() => {
    function onFocus() {
      refetch();
    }
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [refetch]);

  async function handle(
    kind: 'microphone' | 'screenRecording',
    status: 'Unknown' | 'Denied' | 'Granted',
  ) {
    try {
      if (status === 'Unknown') {
        const requestedStatus = await requestCapturePermission(kind);
        await refetch();
        if (requestedStatus === 'Granted') {
          push({ title: 'Permission granted', description: 'Capture access is ready.' });
        } else {
          await openCapturePermissionSettings(kind);
          push({ title: 'Permission needed', description: 'Grant access in System Settings, then return here.' });
        }
      } else {
        await openCapturePermissionSettings(kind);
        push({ title: 'Opening System Settings…', description: 'Grant access, then return to the app.' });
      }
    } catch (error) {
      push({ title: 'Could not update permission', description: String(error), variant: 'destructive' });
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
          onClick={() => handle('microphone', data.microphone)}
        />
        <PermissionRow
          label="Screen recording"
          status={data.screenRecording}
          onClick={() => handle('screenRecording', data.screenRecording)}
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
      <Button
        variant="secondary"
        size="sm"
        aria-label={`${status === 'Unknown' ? 'Request access' : 'Open settings'} for ${label}`}
        onClick={onClick}
      >
        {status === 'Unknown' ? 'Request access' : 'Open settings'}
      </Button>
    </div>
  );
}
