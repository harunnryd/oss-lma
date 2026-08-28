import { useEffect, useRef } from 'react';

import { useTranscriptStore } from '@/stores/transcriptStore';
import { cn } from '@/lib/utils';

export function TranscriptView() {
  const segments = useTranscriptStore((s) => s.segments);
  const ref = useRef<HTMLOListElement | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [segments]);

  if (segments.length === 0) {
    return (
      <div className="grid place-items-center rounded-md border border-dashed border-border py-16 text-center">
        <div className="text-sm text-muted-foreground">
          Transcript will appear here once the meeting is recording.
        </div>
      </div>
    );
  }

  return (
    <ol
      ref={ref}
      aria-live="polite"
      aria-label="Live transcript"
      className="scrollbar-thin max-h-[60vh] space-y-1.5 overflow-y-auto pr-2"
    >
      {segments.map((segment) => (
        <li
          key={segment.segmentId}
          data-segment-id={segment.segmentId}
          className={cn(
            'flex items-start gap-3 rounded-md border border-border bg-card/60 px-3 py-2 text-sm',
            segment.isPartial && 'border-dashed opacity-80',
          )}
        >
          <span className="inline-flex h-6 w-12 shrink-0 items-center justify-center rounded bg-secondary text-[10px] font-mono uppercase text-secondary-foreground">
            {segment.channel === 'CALLER' ? 'Caller' : segment.channel === 'AGENT' ? 'Agent' : 'Asst'}
          </span>
          <span className="flex-1 leading-relaxed">
            {segment.speaker && (
              <span className="mr-2 text-xs font-medium text-muted-foreground">{segment.speaker}</span>
            )}
            {segment.text}
            {segment.isPartial && <span className="ml-1 inline-block h-3 w-1.5 animate-pulse bg-primary align-middle" />}
          </span>
        </li>
      ))}
    </ol>
  );
}
