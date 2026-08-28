import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';


export type CapturePhase = 'Idle' | 'Preflight' | 'Starting' | 'Active' | 'Paused' | 'Stopping' | 'Failed';

export type CaptureStatus = {
  phase: CapturePhase;
  meetingId: string | null;
  errorCode: string | null;
};

type WireCapturePhase = 'idle' | 'preflight' | 'starting' | 'active' | 'paused' | 'stopping' | 'failed';

type WirePermissionStatus = 'unknown' | 'denied' | 'granted';

type WirePermissionSnapshot = {
  screenRecording: WirePermissionStatus;
  microphone: WirePermissionStatus;
};

type WireCaptureSnapshot = {
  phase: WireCapturePhase;
  meetingId: string | null;
  error: string | null;
};

const CAPTURE_PHASES: Record<WireCapturePhase, CapturePhase> = {
  idle: 'Idle',
  preflight: 'Preflight',
  starting: 'Starting',
  active: 'Active',
  paused: 'Paused',
  stopping: 'Stopping',
  failed: 'Failed',
};

const PERMISSION_STATUSES: Record<WirePermissionStatus, PermissionStatus> = {
  unknown: 'Unknown',
  denied: 'Denied',
  granted: 'Granted',
};

export type ProviderKind = 'deepgram' | 'assemblyAi' | 'azure';

export type ProviderSettings = {
  provider: ProviderKind;
  model: string;
  language: string | null;
  azureRegion: string | null;
  hasSecret: boolean;
  diarizeSystem: boolean;
  diarizeMic: boolean;
};

export type ProviderDraft = {
  provider: ProviderKind;
  model: string;
  language: string | null;
  azureRegion: string | null;
  apiKey: string | null;
  diarizeSystem: boolean;
  diarizeMic: boolean;
};

export type PermissionStatus = 'Unknown' | 'Denied' | 'Granted';

export type PermissionSnapshot = {
  screenRecording: PermissionStatus;
  microphone: PermissionStatus;
};

export type MeetingSummary = {
  id: string;
  title: string;
  status: string;
  startedAt: number;
  endedAt: number | null;
  durationMs: number | null;
};

export type MeetingSegment = {
  segmentId: string;
  meetingId: string;
  channel: 'CALLER' | 'AGENT' | 'AGENT_ASSISTANT';
  speaker: string | null;
  startMs: number;
  endMs: number;
  transcript: string;
  isPartial: boolean;
};

export async function captureStatus(): Promise<CaptureStatus> {
  const snapshot = await invoke<WireCaptureSnapshot>('capture_status');
  return {
    phase: decodeWireValue(snapshot.phase, CAPTURE_PHASES, 'capture phase'),
    meetingId: snapshot.meetingId,
    errorCode: snapshot.error,
  };
}

export async function startMeeting(options: { diarizeMicrophone?: boolean } = {}): Promise<void> {
  await invoke('start_meeting', { options });
}

export async function pauseMeeting(): Promise<void> {
  await invoke('pause_meeting');
}

export async function resumeMeeting(): Promise<void> {
  await invoke('resume_meeting');
}

export async function stopMeeting(): Promise<void> {
  await invoke('stop_meeting');
}

export async function capturePermissions(): Promise<PermissionSnapshot> {
  const snapshot = await invoke<WirePermissionSnapshot>('capture_permissions');
  return {
    screenRecording: decodeWireValue(snapshot.screenRecording, PERMISSION_STATUSES, 'screen-recording permission'),
    microphone: decodeWireValue(snapshot.microphone, PERMISSION_STATUSES, 'microphone permission'),
  };
}

export async function requestCapturePermission(
  kind: 'microphone' | 'screenRecording',
): Promise<PermissionStatus> {
  const status = await invoke<WirePermissionStatus>('request_capture_permission', { kind });
  return decodeWireValue(status, PERMISSION_STATUSES, 'permission status');
}

export async function openCapturePermissionSettings(kind: 'microphone' | 'screenRecording'): Promise<void> {
  await invoke('open_capture_permission_settings', { kind });
}

export async function getProviderSettings(): Promise<ProviderSettings> {
  return invoke<ProviderSettings>('provider_settings');
}

export async function saveProviderSettings(draft: ProviderDraft): Promise<ProviderSettings> {
  const { apiKey, ...settings } = draft;
  return invoke<ProviderSettings>('save_provider_settings', {
    settings,
    secret: apiKey,
  });
}

export async function listMeetings(limit = 50): Promise<MeetingSummary[]> {
  return invoke<MeetingSummary[]>('list_meetings', { limit });
}

export async function listMeetingSegments(meetingId: string): Promise<MeetingSegment[]> {
  return invoke<MeetingSegment[]>('list_meeting_segments', { meetingId });
}

export async function deleteMeeting(meetingId: string): Promise<void> {
  await invoke('delete_meeting', { meetingId });
}

export type WireEvent =
  | { EventType: 'ADD_TRANSCRIPT_SEGMENT'; CallId: string; SegmentId: string; Channel: string; Speaker?: string; StartTime: number; EndTime: number; Transcript: string; IsPartial: boolean }
  | { EventType: 'DELETE_TRANSCRIPT_SEGMENT'; CallId: string; SegmentId: string; Reason?: string }
  | { EventType: 'SPEAKER_CHANGE'; CallId: string; Channel: string; ActiveSpeaker: string }
  | { EventType: 'PAUSE'; CallId: string }
  | { EventType: 'RESUME'; CallId: string }
  | { EventType: 'END'; CallId: string }
  | { EventType: 'START'; CallId: string; SamplingRate: number; DiarizeSystemChannel?: boolean; DiarizeMicChannel?: boolean }
  | { EventType: 'THINKING_STEP'; CallId: string; QueryId: string; Seq: number; StepType: string; Content?: string }
  | { EventType: 'ERROR'; CallId: string; Code: string; Context?: Record<string, unknown> };

export function onMeetingEvent(handler: (event: WireEvent) => void): Promise<UnlistenFn> {
  return listen<WireEvent>('meeting-event', (e) => handler(e.payload));
}

export function onCaptureStatus(handler: (status: CaptureStatus) => void): Promise<UnlistenFn> {
  return listen<WireCaptureSnapshot>('capture-status', (e) => handler({
    phase: decodeWireValue(e.payload.phase, CAPTURE_PHASES, 'capture phase'),
    meetingId: e.payload.meetingId,
    errorCode: e.payload.error,
  }));
}

function decodeWireValue<T extends string>(value: string, values: Record<string, T>, label: string): T {
  const decoded = values[value];
  if (!decoded) {
    throw new Error(`unsupported ${label}: ${value}`);
  }
  return decoded;
}
