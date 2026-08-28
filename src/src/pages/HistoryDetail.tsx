import { Link, useParams } from 'react-router-dom';
import { ArrowLeft } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { useMeetingSegments, useMeetings } from '@/hooks/useTauriCommand';
import { formatDuration, formatRelativeTime } from '@/lib/utils';

export function HistoryDetailPage() {
  const { meetingId } = useParams<{ meetingId: string }>();
  const { data: meetings } = useMeetings(100);
  const { data: segments, isLoading } = useMeetingSegments(meetingId ?? null);

  const meeting = meetings?.find((m) => m.id === meetingId);

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <div>
        <Button variant="ghost" asChild className="px-2">
          <Link to="/history">
            <ArrowLeft className="h-4 w-4" /> Back to history
          </Link>
        </Button>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">
            {meeting?.title || (meetingId ? `Meeting ${meetingId.slice(0, 8)}` : 'Meeting')}
          </CardTitle>
          <CardDescription>
            {meeting
              ? `${formatRelativeTime(meeting.startedAt)} · ${formatDuration(meeting.startedAt, meeting.endedAt)}`
              : 'Loading meeting metadata…'}
          </CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <p className="text-sm text-muted-foreground">Loading transcript…</p>
          ) : !segments || segments.length === 0 ? (
            <p className="text-sm text-muted-foreground">No transcript segments stored.</p>
          ) : (
            <ol className="space-y-1.5">
              {segments.map((segment) => (
                <li
                  key={segment.segmentId}
                  className="flex items-start gap-3 rounded-md border border-border bg-card/60 px-3 py-2 text-sm"
                >
                  <span className="inline-flex h-6 w-14 shrink-0 items-center justify-center rounded bg-secondary text-[10px] font-mono uppercase text-secondary-foreground">
                    {segment.channel}
                  </span>
                  <span className="flex-1 leading-relaxed">
                    {segment.speaker && (
                      <span className="mr-2 text-xs font-medium text-muted-foreground">{segment.speaker}</span>
                    )}
                    {segment.transcript}
                  </span>
                </li>
              ))}
            </ol>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
