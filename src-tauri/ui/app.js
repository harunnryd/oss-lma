(() => {
  const { core, event } = window.__TAURI__ || {};
  const invoke = core?.invoke;
  const listen = event?.listen;
  const $ = selector => document.querySelector(selector);
  const phase = $('#phase');
  const message = $('#message');
  const transcript = $('#transcript');
  const setMessage = value => { message.textContent = value || ''; };
  const call = async (command, args) => {
    if (!invoke) throw new Error('The desktop backend is unavailable.');
    return invoke(command, args);
  };
  const recoveryMessage = error => ({
    CAPTURE_PERMISSION_DENIED: 'Grant microphone and screen recording access, then try again.',
    CAPTURE_DEVICE_LOST: 'Reconnect the missing audio device, then start a new meeting.',
    STT_PROVIDER_AUTH: 'Check the provider API key in Transcription provider.',
    STT_STREAM_RESET: 'The transcription stream reset. It will reconnect automatically.',
    SIDECAR_UNAVAILABLE: 'The transcription service is unavailable. Save provider settings and try again.',
    LINK_DISCONNECTED: 'The transcription connection dropped. Stop and start a new meeting.',
    VP_CONTAINER_FAILED: 'The virtual participant failed. Restart the participant and try again.',
    VP_MANUAL_ACTION_REQUIRED: 'The virtual participant needs your attention. Open its takeover controls.',
    AGENT_TOOL_FAILURE: 'An assistant action failed. The meeting can continue.',
    RAG_EMBEDDING_UNAVAILABLE: 'Knowledge indexing is unavailable. Try the import again later.',
    DB_WRITE_CONFLICT: 'Saving conflicted with another update. Try again.',
    PORT_BIND_FAILED: 'The transcription service could not claim a port. Try again.',
  }[error] || error);
  const updateControls = snapshot => {
    const current = snapshot.phase;
    phase.textContent = current;
    const active = ['active', 'paused'].includes(current);
    $('#start').disabled = active || ['preflight', 'starting', 'stopping'].includes(current);
    $('#pause').disabled = current !== 'active';
    $('#resume').disabled = current !== 'paused';
    $('#stop').disabled = !active;
    if (snapshot.error) setMessage(recoveryMessage(snapshot.error));
  };
  const updateTranscript = envelope => {
    if (envelope.EventType !== 'ADD_TRANSCRIPT_SEGMENT' || !envelope.SegmentId) return false;
    let row = transcript.querySelector(`[data-segment-id="${CSS.escape(envelope.SegmentId)}"]`);
    if (!row) {
      row = document.createElement('li');
      row.dataset.segmentId = envelope.SegmentId;
      transcript.append(row);
    }
    row.textContent = envelope.Transcript || '';
    row.classList.toggle('partial', envelope.IsPartial === true);
    return true;
  };
  const refreshPermissions = async () => {
    const status = await call('capture_permissions');
    $('#permission-status').textContent = `Microphone: ${status.microphone}; screen recording: ${status.screenRecording}`;
  };
  const refreshProviderSettings = async () => {
    const settings = await call('provider_settings');
    $('#provider').value = settings.provider;
    $('#model').value = settings.model;
    $('#language').value = settings.language || '';
    $('#azure-region').value = settings.azureRegion || '';
    $('#diarize-mic').checked = settings.diarizeMic;
    $('#secret-status').textContent = settings.hasSecret ? 'API key stored securely' : 'No API key stored';
    $('#azure-region-field').hidden = settings.provider !== 'azure';
  };
  const refresh = async () => {
    try {
      updateControls(await call('capture_status'));
      await Promise.all([refreshPermissions(), refreshProviderSettings()]);
    } catch (error) { setMessage(String(error)); }
  };
  $('#provider').addEventListener('change', () => { $('#azure-region-field').hidden = $('#provider').value !== 'azure'; });
  $('#permissions').onclick = () => call('open_capture_permission_settings', { kind: 'microphone' }).then(refreshPermissions).catch(error => setMessage(String(error)));
  $('#screen').onclick = () => call('open_capture_permission_settings', { kind: 'screenRecording' }).then(refreshPermissions).catch(error => setMessage(String(error)));
  $('#provider-form').onsubmit = async event => {
    event.preventDefault();
    try {
      await call('save_provider_settings', { settings: { provider: $('#provider').value, model: $('#model').value, language: $('#language').value || null, azureRegion: $('#azure-region').value || null, diarizeSystem: false, diarizeMic: $('#diarize-mic').checked }, secret: $('#provider-secret').value || null });
      $('#provider-secret').value = '';
      await refreshProviderSettings();
      setMessage('Provider settings saved.');
    } catch (error) { setMessage(String(error)); }
  };
  $('#start').onclick = () => call('start_meeting', { options: { diarizeMicrophone: $('#diarize-mic').checked } }).then(updateControls).catch(error => setMessage(recoveryMessage(String(error))));
  $('#pause').onclick = () => call('pause_meeting').then(updateControls).catch(error => setMessage(String(error)));
  $('#resume').onclick = () => call('resume_meeting').then(updateControls).catch(error => setMessage(String(error)));
  $('#stop').onclick = () => call('stop_meeting').then(updateControls).catch(error => setMessage(String(error)));
  if (listen) {
    listen('capture-status', ({ payload }) => updateControls(payload));
    listen('meeting-event', ({ payload }) => {
      if (payload.EventType === 'ADD_TRANSCRIPT_SEGMENT') updateTranscript(payload);
      if (payload.EventType === 'ERROR') setMessage(recoveryMessage(payload.Code));
    });
  }
  window.ossLma = { updateTranscript, recoveryMessage };
  refresh();
})();
