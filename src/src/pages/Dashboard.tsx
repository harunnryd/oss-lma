import { Link } from 'react-router-dom';
import { ArrowRight, History as HistoryIcon, Mic, PlayCircle, Settings } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Separator } from '@/components/ui/separator';
import { StatusBadge } from '@/components/layout/StatusBadge';
import { useCaptureStatus, useMeetings, usePermissions, useProviderSettings } from '@/hooks/useTauriCommand';
import { formatRelativeTime } from '@/lib/utils';
import { useMeetingStore } from '@/stores/meetingStore';

export function DashboardPage() {
  const phase = useMeetingStore((s) => s.phase);
  const { data: meetings } = useMeetings(3);
  const { data: permissions } = usePermissions();
  const { data: provider } = useProviderSettings();
  useCaptureStatus();

  const allGranted = permissions?.microphone === 'Granted' && permissions?.screenRecording === 'Granted';
  const providerReady = provider?.hasSecret ?? false;

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <header className="flex flex-col gap-2">
        <h1 className="text-2xl font-semibold tracking-tight">Welcome back</h1>
        <p className="text-sm text-muted-foreground">
          Capture meetings locally, transcribe with your chosen provider, and store everything on this device.
        </p>
      </header>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between gap-4 space-y-0">
          <div>
            <CardTitle className="text-base">Current state</CardTitle>
            <CardDescription>Snapshot of the local capture pipeline.</CardDescription>
          </div>
          <StatusBadge phase={phase} />
        </CardHeader>
        <CardContent className="grid gap-4 sm:grid-cols-3">
          <StatusCell
            icon={<Mic className="h-4 w-4" />}
            label="Permissions"
            value={allGranted ? 'Granted' : 'Action needed'}
            tone={allGranted ? 'success' : 'warning'}
            to="/onboarding"
          />
          <StatusCell
            icon={<Settings className="h-4 w-4" />}
            label="Provider"
            value={providerReady ? `${provider?.provider ?? 'configured'} · ${provider?.model ?? ''}` : 'Not configured'}
            tone={providerReady ? 'success' : 'warning'}
            to="/settings"
          />
          <StatusCell
            icon={<PlayCircle className="h-4 w-4" />}
            label="Meeting"
            value={phase === 'Idle' ? 'Ready to start' : phase}
            tone={phase === 'Idle' ? 'muted' : phase === 'Failed' ? 'destructive' : 'success'}
            to="/live"
          />
        </CardContent>
      </Card>

      <div className="grid gap-6 lg:grid-cols-[2fr_1fr]">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
            <div>
              <CardTitle className="text-base">Recent meetings</CardTitle>
              <CardDescription>Last three captures on this device.</CardDescription>
            </div>
            <Button variant="ghost" size="sm" asChild>
              <Link to="/history">
                See all <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>
          </CardHeader>
          <CardContent className="space-y-2">
            {!meetings || meetings.length === 0 ? (
              <p className="text-sm text-muted-foreground">No meetings yet. Start one from the Live tab.</p>
            ) : (
              meetings.map((m, idx) => (
                <div key={m.id}>
                  {idx > 0 && <Separator className="my-2" />}
                  <Link
                    to={`/history/${m.id}`}
                    className="flex items-center justify-between rounded-md px-2 py-2 transition-colors hover:bg-accent"
                  >
                    <div className="flex flex-col">
                      <span className="text-sm font-medium">{m.title || `Meeting ${m.id.slice(0, 8)}`}</span>
                      <span className="text-xs text-muted-foreground">{formatRelativeTime(m.startedAt)}</span>
                    </div>
                    <span className="text-xs uppercase tracking-wide text-muted-foreground">{m.status}</span>
                  </Link>
                </div>
              ))
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Quick start</CardTitle>
            <CardDescription>One click once permissions and provider are configured.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-2">
            <Button asChild disabled={!allGranted || !providerReady}>
              <Link to="/live">
                <Mic className="h-4 w-4" /> Open Live meeting
              </Link>
            </Button>
            <Button variant="secondary" asChild>
              <Link to="/settings">
                <Settings className="h-4 w-4" /> Configure provider
              </Link>
            </Button>
            <Button variant="ghost" asChild>
              <Link to="/history">
                <HistoryIcon className="h-4 w-4" /> Browse history
              </Link>
            </Button>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function StatusCell({
  icon,
  label,
  value,
  tone,
  to,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  tone: 'success' | 'warning' | 'destructive' | 'muted';
  to: string;
}) {
  const toneClass =
    tone === 'success'
      ? 'text-emerald-400'
      : tone === 'warning'
        ? 'text-amber-300'
        : tone === 'destructive'
          ? 'text-destructive'
          : 'text-muted-foreground';
  return (
    <Link
      to={to}
      className="flex items-start gap-3 rounded-md border border-border bg-card/40 p-3 transition-colors hover:bg-accent/40"
    >
      <span className={toneClass}>{icon}</span>
      <div className="flex flex-col leading-tight">
        <span className="text-xs uppercase tracking-wide text-muted-foreground">{label}</span>
        <span className="text-sm font-medium">{value}</span>
      </div>
    </Link>
  );
}
