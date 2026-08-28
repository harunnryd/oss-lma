import { AlertCircle } from 'lucide-react';

import { useMeetingStore } from '@/stores/meetingStore';
import { recoveryMessage } from '@/lib/recovery';

export function RecoveryBanner() {
  const errorCode = useMeetingStore((s) => s.errorCode);
  if (!errorCode) return null;
  return (
    <div
      role="alert"
      className="flex items-start gap-3 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
    >
      <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
      <div className="flex-1">
        <div className="font-medium">Recovery needed</div>
        <div className="text-destructive/80">{recoveryMessage(errorCode)}</div>
      </div>
    </div>
  );
}
