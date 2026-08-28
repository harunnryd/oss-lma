// Mirrors the error codes declared in contracts/errors.yaml. Adding
// a new code on the Rust side without updating this map will surface
// the raw code in the UI as a fallback.

const CATALOG: Record<string, string> = {
  STT_PROVIDER_AUTH: 'The transcription provider rejected the API key. Update it in Settings.',
  STT_STREAM_RESET: 'The transcription stream reset. It will reconnect automatically.',
  LINK_DISCONNECTED: 'The connection to the sidecar was lost. Reconnecting…',
  CAPTURE_DEVICE_LOST: 'Capture device was lost. Check microphone and system audio routing.',
  CAPTURE_PERMISSION_DENIED: 'Microphone or screen-recording permission was denied. Open Settings to grant access.',
  VP_CONTAINER_FAILED: 'The virtual participant container failed to start. Check Docker.',
  VP_MANUAL_ACTION_REQUIRED: 'The virtual participant needs manual action. Open the dashboard.',
  AGENT_TOOL_FAILURE: 'The meeting assistant hit a tool error. Try again.',
  RAG_EMBEDDING_UNAVAILABLE: 'Retrieval search is offline. Embedding service unavailable.',
  DB_WRITE_CONFLICT: 'A database write conflicted with another writer. Retrying.',
  SIDECAR_UNAVAILABLE: 'The sidecar is offline. Restart the app or check the supervisor.',
  PORT_BIND_FAILED: 'Could not bind a port for the sidecar. Another process may be using it.',
};

export function recoveryMessage(code: string | null | undefined): string {
  if (!code) return '';
  return CATALOG[code] ?? code;
}

export const ALL_RECOVERY_CODES = Object.keys(CATALOG);
