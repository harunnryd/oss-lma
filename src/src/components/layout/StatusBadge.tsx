import { Badge } from '@/components/ui/badge';
import type { CapturePhase } from '@/lib/tauri';

const PHASE_LABEL: Record<CapturePhase, string> = {
  Idle: 'Idle',
  Preflight: 'Preflight',
  Starting: 'Starting',
  Active: 'Recording',
  Paused: 'Paused',
  Stopping: 'Stopping',
  Failed: 'Failed',
};

export function StatusBadge({ phase }: { phase: CapturePhase }) {
  const variant =
    phase === 'Active' ? 'success' : phase === 'Failed' ? 'destructive' : phase === 'Paused' ? 'warning' : 'secondary';
  return <Badge variant={variant as never}>{PHASE_LABEL[phase]}</Badge>;
}
