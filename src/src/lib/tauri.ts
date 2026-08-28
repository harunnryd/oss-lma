import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// ----- Tauri command wrappers -----
// These match the Rust commands exposed by crates/app/src/commands/*.

export type CapturePhase = 'Idle' | 'Preflight' | 'Starting' | 'Active' | 'Paused' | 'Stopping' | 'Failed';

export type CaptureStatus = {
  phase: CapturePhase;
  meetingId: string | null;
  errorCode: string | null;
};

export type ProviderKind = 'Deepgram' | 'AssemblyAi' | 'Azure';

export type ProviderSettings = {
  provider: ProviderKind;
  model: string;
  language: string | null;
  azureRegion: string | null;
  hasSecret: boolean;
  diarizeMic: boolean;
};

export type ProviderDraft = {
  provider: ProviderKind;
  model: string;
  language: string | null;
  azureRegion: string | null;
  apiKey: string | null;
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
  return invoke<CaptureStatus>('capture_status');
}

export async function startMeeting(): Promise<void> {
  await invoke('start_meeting');
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
  return invoke<PermissionSnapshot>('capture_permissions');
}

export async function openCapturePermissionSettings(kind: 'microphone' | 'screenRecording'): Promise<void> {
  await invoke('open_capture_permission_settings', { kind });
}

export async function getProviderSettings(): Promise<ProviderSettings> {
  return invoke<ProviderSettings>('provider_settings');
}

export async function saveProviderSettings(draft: ProviderDraft): Promise<void> {
  await invoke('save_provider_settings', { draft });
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

// ----- Wire events -----
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
  return listen<CaptureStatus>('capture-status', (e) => handler(e.payload));
}
