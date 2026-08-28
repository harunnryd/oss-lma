import { useEffect, useState } from 'react';
import { Save } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { useProviderSettings, useSaveProviderSettings } from '@/hooks/useTauriCommand';
import { useToast } from '@/components/ui/toaster';
import type { ProviderDraft, ProviderSettings } from '@/lib/tauri';

const PROVIDERS: Array<{ value: ProviderSettings['provider']; label: string }> = [
  { value: 'Deepgram', label: 'Deepgram (reference)' },
  { value: 'AssemblyAi', label: 'AssemblyAI' },
  { value: 'Azure', label: 'Azure Speech' },
];

export function ProviderForm() {
  const { data, isLoading } = useProviderSettings();
  const save = useSaveProviderSettings();
  const { push } = useToast();
  const [draft, setDraft] = useState<ProviderDraft>({
    provider: 'Deepgram',
    model: 'nova-3',
    language: 'en',
    azureRegion: '',
    apiKey: null,
    diarizeMic: true,
  });

  useEffect(() => {
    if (!data) return;
    setDraft((prev) => ({
      ...prev,
      provider: data.provider,
      model: data.model,
      language: data.language ?? '',
      azureRegion: data.azureRegion ?? '',
      apiKey: null,
      diarizeMic: data.diarizeMic,
    }));
  }, [data]);

  const isAzure = draft.provider === 'Azure';

  function handleSave(event: React.FormEvent) {
    event.preventDefault();
    save.mutate(draft, {
      onSuccess: () =>
        push({
          title: 'Provider saved',
          description: data?.hasSecret
            ? 'Settings updated. New key will be used on next meeting.'
            : 'Settings saved. Enter the API key to enable transcription.',
          variant: 'default',
        }),
      onError: (error) =>
        push({ title: 'Save failed', description: String(error), variant: 'destructive' }),
    });
  }

  if (isLoading) {
    return (
      <Card>
        <CardContent className="py-8 text-center text-sm text-muted-foreground">Loading provider settings…</CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Transcription provider</CardTitle>
        <CardDescription>
          {data?.hasSecret
            ? 'API key stored securely in the OS keychain. Leave the field blank to keep the existing key.'
            : 'No API key stored yet. Enter one to enable transcription.'}
        </CardDescription>
      </CardHeader>
      <form onSubmit={handleSave}>
        <CardContent className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="provider">Provider</Label>
            <Select
              value={draft.provider}
              onValueChange={(value) =>
                setDraft((prev) => ({ ...prev, provider: value as ProviderSettings['provider'] }))
              }
            >
              <SelectTrigger id="provider">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PROVIDERS.map((p) => (
                  <SelectItem key={p.value} value={p.value}>
                    {p.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="grid gap-2 sm:grid-cols-2">
            <div className="grid gap-2">
              <Label htmlFor="model">Model</Label>
              <Input
                id="model"
                value={draft.model}
                onChange={(e) => setDraft((prev) => ({ ...prev, model: e.target.value }))}
                required
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="language">Language (optional)</Label>
              <Input
                id="language"
                placeholder="en"
                value={draft.language ?? ''}
                onChange={(e) => setDraft((prev) => ({ ...prev, language: e.target.value || null }))}
              />
            </div>
          </div>

          {isAzure && (
            <div className="grid gap-2">
              <Label htmlFor="azure-region">Azure region</Label>
              <Input
                id="azure-region"
                placeholder="eastus"
                value={draft.azureRegion ?? ''}
                onChange={(e) => setDraft((prev) => ({ ...prev, azureRegion: e.target.value || null }))}
                required
              />
            </div>
          )}

          <div className="grid gap-2">
            <Label htmlFor="api-key">API key</Label>
            <Input
              id="api-key"
              type="password"
              autoComplete="off"
              placeholder="Stored in OS keychain"
              value={draft.apiKey ?? ''}
              onChange={(e) => setDraft((prev) => ({ ...prev, apiKey: e.target.value || null }))}
            />
            <p className="text-xs text-muted-foreground">
              The key never leaves this device. It is passed to the sidecar over a private inherited pipe and is not
              logged or persisted in SQLite.
            </p>
          </div>

          <div className="flex items-center gap-2 pt-2">
            <Checkbox
              id="diarize-mic"
              checked={draft.diarizeMic}
              onCheckedChange={(checked) => setDraft((prev) => ({ ...prev, diarizeMic: checked }))}
            />
            <Label htmlFor="diarize-mic" className="cursor-pointer">
              Separate microphone speaker labels
            </Label>
          </div>
        </CardContent>
        <CardFooter className="justify-end">
          <Button type="submit" disabled={save.isPending}>
            <Save className="h-4 w-4" /> {save.isPending ? 'Saving…' : 'Save provider settings'}
          </Button>
        </CardFooter>
      </form>
    </Card>
  );
}
