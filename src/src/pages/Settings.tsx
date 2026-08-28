import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { PermissionGate } from '@/components/onboarding/PermissionGate';
import { ProviderForm } from '@/components/provider/ProviderForm';

export function SettingsPage() {
  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-6">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">
          Re-grant permissions, change provider, or rotate the API key. Secrets never leave this device.
        </p>
      </header>

      <PermissionGate />
      <ProviderForm />

      <Card>
        <CardHeader>
          <CardTitle className="text-base">About secrets</CardTitle>
          <CardDescription>How oss-lma keeps your provider keys safe.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-2 text-sm text-muted-foreground">
          <p>
            API keys are stored in the macOS keychain and read only at sidecar spawn. They are passed to the sidecar
            over a private inherited pipe — never as a CLI argument, never in a SQLite row, never in a log line.
          </p>
          <p>
            Leave the API key field blank to keep the existing key. Enter a new value to rotate.
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
