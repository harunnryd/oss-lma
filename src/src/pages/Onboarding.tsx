import { Link } from 'react-router-dom';
import { ArrowRight } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { PermissionGate } from '@/components/onboarding/PermissionGate';
import { ProviderForm } from '@/components/provider/ProviderForm';
import { usePermissions, useProviderSettings } from '@/hooks/useTauriCommand';
import { cn } from '@/lib/utils';

export function OnboardingPage() {
  const { data: permissions } = usePermissions();
  const { data: provider } = useProviderSettings();

  const permsGranted =
    permissions?.microphone === 'Granted' && permissions?.screenRecording === 'Granted';
  const providerReady = provider?.hasSecret ?? false;
  const allReady = permsGranted && providerReady;

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-6">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">First-time setup</h1>
        <p className="text-sm text-muted-foreground">
          Grant capture access and configure a transcription provider. You can revisit these from Settings.
        </p>
      </header>

      <Step n={1} title="Grant system permissions" done={permsGranted}>
        <PermissionGate />
      </Step>

      <Step n={2} title="Configure a transcription provider" done={providerReady}>
        <ProviderForm />
      </Step>

      <Card className={cn(allReady ? 'border-emerald-500/40 bg-emerald-500/5' : 'border-dashed')}>
        <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
          <div>
            <CardTitle className="text-base">Ready to capture</CardTitle>
            <CardDescription>
              {allReady
                ? 'Everything is configured. Head to the Live tab to start a meeting.'
                : 'Complete the steps above to enable live capture.'}
            </CardDescription>
          </div>
          <Button asChild disabled={!allReady}>
            <Link to="/live">
              Go to Live <ArrowRight className="h-4 w-4" />
            </Link>
          </Button>
        </CardHeader>
      </Card>
    </div>
  );
}

function Step({ n, title, done, children }: { n: number; title: string; done: boolean; children: React.ReactNode }) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
        <CardTitle className="flex items-center gap-3 text-base">
          <span
            className={cn(
              'grid h-7 w-7 place-items-center rounded-full text-xs font-semibold',
              done ? 'bg-emerald-500/20 text-emerald-300' : 'bg-secondary text-secondary-foreground',
            )}
          >
            {done ? '✓' : n}
          </span>
          {title}
        </CardTitle>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
}
