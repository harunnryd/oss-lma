import { Link } from 'react-router-dom';
import { Trash2 } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { useDeleteMeeting, useMeetings } from '@/hooks/useTauriCommand';
import { useToast } from '@/components/ui/toast-context';
import { formatDuration, formatRelativeTime } from '@/lib/utils';
import { StatusBadge } from '@/components/layout/StatusBadge';
import type { CapturePhase } from '@/lib/tauri';

export function HistoryPage() {
  const { data: meetings, isLoading } = useMeetings(100);
  const del = useDeleteMeeting();
  const { push } = useToast();

  function handleDelete(meetingId: string, title: string) {
    if (!window.confirm(`Delete "${title}"? This cannot be undone.`)) return;
    del.mutate(meetingId, {
      onSuccess: () => push({ title: 'Meeting deleted', description: title }),
      onError: (error) =>
        push({ title: 'Could not delete', description: String(error), variant: 'destructive' }),
    });
  }

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight">History</h1>
        <p className="text-sm text-muted-foreground">
          Every meeting captured on this device. Click any row to view its transcript.
        </p>
      </header>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">All meetings</CardTitle>
          <CardDescription>{meetings?.length ?? 0} total on this device.</CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <p className="text-sm text-muted-foreground">Loading…</p>
          ) : !meetings || meetings.length === 0 ? (
            <p className="text-sm text-muted-foreground">No meetings yet.</p>
          ) : (
            <div className="divide-y divide-border">
              {meetings.map((m) => (
                <div key={m.id} className="flex items-center gap-4 py-3">
                  <div className="flex flex-1 flex-col">
                    <Link to={`/history/${m.id}`} className="text-sm font-medium hover:underline">
                      {m.title || `Meeting ${m.id.slice(0, 8)}`}
                    </Link>
                    <div className="mt-0.5 flex items-center gap-2 text-xs text-muted-foreground">
                      <span>{formatRelativeTime(m.startedAt)}</span>
                      <span>·</span>
                      <span>{formatDuration(m.startedAt, m.endedAt)}</span>
                    </div>
                  </div>
                  <StatusBadge phase={(m.status === 'RECORDING' ? 'Active' : m.status === 'COMPLETED' ? 'Idle' : 'Failed') as CapturePhase} />
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={`Delete ${m.title || m.id}`}
                    onClick={() => handleDelete(m.id, m.title || `Meeting ${m.id.slice(0, 8)}`)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
