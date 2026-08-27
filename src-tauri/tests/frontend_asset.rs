use std::fs;
use std::process::Command;

#[test]
fn tauri_frontend_dist_contains_index_html() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("tauri.conf.json")).expect("tauri config exists"),
    )
    .expect("tauri config is valid json");
    let frontend_dist = config["build"]["frontendDist"]
        .as_str()
        .expect("frontendDist is configured");
    assert!(root.join(frontend_dist).join("index.html").is_file());
}

#[test]
fn frontend_contains_provider_settings_and_live_transcript_surfaces() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let index = fs::read_to_string(root.join("ui/index.html")).expect("index exists");
    let app = fs::read_to_string(root.join("ui/app.js")).expect("app exists");

    for required_surface in [
        "id=\"provider-settings\"",
        "id=\"provider\"",
        "id=\"provider-secret\"",
        "id=\"permission-status\"",
        "id=\"transcript\"",
        "id=\"start\"",
        "id=\"pause\"",
        "id=\"resume\"",
        "id=\"stop\"",
    ] {
        assert!(
            index.contains(required_surface),
            "missing {required_surface}"
        );
    }

    for required_behavior in [
        "updateTranscript",
        "SegmentId",
        "IsPartial",
        "ADD_TRANSCRIPT_SEGMENT",
        "capture-status",
        "provider_settings",
    ] {
        assert!(
            app.contains(required_behavior),
            "missing {required_behavior}"
        );
    }

    for forbidden_detail in ["port: 8765", "token:", "endpoint"] {
        assert!(
            !index.contains(forbidden_detail),
            "contains {forbidden_detail}"
        );
        assert!(
            !app.contains(forbidden_detail),
            "contains {forbidden_detail}"
        );
    }
}

#[test]
fn transcript_updates_replace_partial_segments_with_final_segments() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = r#"
const fs = require('fs');
const rows = [];
const element = () => ({ value: '', checked: false, hidden: false, textContent: '', disabled: false, dataset: {}, classList: { toggle() {} }, addEventListener() {} });
const byId = new Map();
for (const id of ['phase', 'message', 'transcript', 'provider', 'model', 'language', 'azure-region', 'diarize-mic', 'secret-status', 'azure-region-field', 'permissions', 'screen', 'provider-form', 'start', 'pause', 'resume', 'stop', 'provider-secret', 'permission-status']) byId.set(`#${id}`, element());
byId.get('#transcript').append = row => rows.push(row);
byId.get('#transcript').querySelector = selector => rows.find(row => selector.includes(row.dataset.segmentId));
const listeners = {};
global.window = { __TAURI__: { event: { listen: (name, handler) => { listeners[name] = handler; return Promise.resolve(() => {}); } } }, ossLma: undefined };
global.document = { querySelector: selector => byId.get(selector), createElement: () => element() };
global.CSS = { escape: value => value };
eval(fs.readFileSync(process.argv[1], 'utf8'));
if (!window.ossLma.updateTranscript({ EventType: 'ADD_TRANSCRIPT_SEGMENT', SegmentId: 's1', Transcript: 'partial', IsPartial: true })) process.exit(1);
if (!window.ossLma.updateTranscript({ EventType: 'ADD_TRANSCRIPT_SEGMENT', SegmentId: 's1', Transcript: 'final', IsPartial: false })) process.exit(1);
if (rows.length !== 1 || rows[0].textContent !== 'final') process.exit(1);
const codes = ['STT_PROVIDER_AUTH', 'STT_STREAM_RESET', 'LINK_DISCONNECTED', 'CAPTURE_DEVICE_LOST', 'CAPTURE_PERMISSION_DENIED', 'VP_CONTAINER_FAILED', 'VP_MANUAL_ACTION_REQUIRED', 'AGENT_TOOL_FAILURE', 'RAG_EMBEDDING_UNAVAILABLE', 'DB_WRITE_CONFLICT', 'SIDECAR_UNAVAILABLE', 'PORT_BIND_FAILED'];
if (codes.some(code => window.ossLma.recoveryMessage(code) === code)) process.exit(1);
if (window.ossLma.recoveryMessage('UNKNOWN_CODE') !== 'UNKNOWN_CODE') process.exit(1);
listeners['meeting-event']({ payload: { EventType: 'ERROR', CallId: 'call-1', Code: 'STT_STREAM_RESET', Context: {} } });
if (byId.get('#message').textContent !== 'The transcription stream reset. It will reconnect automatically.') process.exit(1);
if (rows.length !== 1) process.exit(1);
"#;
    let output = Command::new("node")
        .arg("-e")
        .arg(fixture)
        .arg(root.join("ui/app.js"))
        .output()
        .expect("node is available for the browserless frontend fixture");
    assert!(output.status.success(), "fixture failed: {output:?}");
}
