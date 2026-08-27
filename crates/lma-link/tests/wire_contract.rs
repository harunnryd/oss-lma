use std::{collections::HashMap, time::Duration};

use futures_util::StreamExt;
use jsonschema::Validator;
use lma_link::{LinkClient, LinkEvent};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::mpsc, time::timeout};
use tokio_tungstenite::{accept_async, tungstenite::Message};

const CALL_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

fn control_validator() -> Validator {
    let mut schema: Value =
        serde_json::from_str(include_str!("../../../contracts/events.schema.json"))
            .expect("event schema parses");
    schema["$defs"]
        .as_object_mut()
        .expect("schema definitions")
        .remove("Error");
    schema["oneOf"]
        .as_array_mut()
        .expect("schema event variants")
        .retain(|event| event["$ref"] != "#/$defs/Error");
    jsonschema::validator_for(&schema).expect("event schema compiles")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn emitted_control_events_satisfy_the_canonical_schema() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fake sidecar binds");
    let port = listener.local_addr().expect("listener address").port();
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    let sidecar = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("link connects");
        let mut socket = accept_async(stream).await.expect("websocket upgrades");
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame.expect("valid websocket frame") else {
                continue;
            };
            let event: Value = serde_json::from_str(&text).expect("control event is JSON");
            let ended = event["EventType"] == "END";
            events_tx.send(event).expect("test receiver remains open");
            if ended {
                break;
            }
        }
    });

    let client = LinkClient::new();
    let mut link_events = client.subscribe();
    client
        .start(CALL_ID, port, "test token", 48_000, true)
        .await
        .expect("link start is accepted");

    let start = timeout(Duration::from_secs(2), events_rx.recv())
        .await
        .expect("START arrives")
        .expect("sidecar remains active");
    assert_eq!(
        timeout(Duration::from_secs(2), link_events.recv())
            .await
            .expect("connected event arrives")
            .expect("event receiver remains active"),
        LinkEvent::Connected
    );
    client.pause().expect("pause is queued");
    client.resume().expect("resume is queued");
    client.end().expect("end is queued");

    let pause = events_rx.recv().await.expect("PAUSE arrives");
    let resume = events_rx.recv().await.expect("RESUME arrives");
    let end = events_rx.recv().await.expect("END arrives");
    sidecar.await.expect("fake sidecar completes");

    let events = [start, pause, resume, end];
    let validator = control_validator();
    for event in &events {
        let errors = validator.iter_errors(event).collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "invalid control event {event}: {errors:?}"
        );
        assert!(event.get("call_id").is_none());
    }
    assert_eq!(
        events,
        [
            json!({
                "EventType": "START",
                "CallId": CALL_ID,
                "SamplingRate": 48_000,
                "DiarizeSystemChannel": false,
                "DiarizeMicChannel": true
            }),
            json!({"EventType": "PAUSE", "CallId": CALL_ID}),
            json!({"EventType": "RESUME", "CallId": CALL_ID}),
            json!({"EventType": "END", "CallId": CALL_ID}),
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn link_loss_emits_connected_then_disconnected() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fake sidecar binds");
    let port = listener.local_addr().expect("listener address").port();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("link connects");
        let mut socket = accept_async(stream).await.expect("websocket upgrades");
        socket
            .next()
            .await
            .expect("START arrives")
            .expect("valid START");
        socket.close(None).await.expect("fake disconnect succeeds");
    });

    let client = LinkClient::new();
    let mut events = client.subscribe();
    client
        .start(CALL_ID, port, "test token", 48_000, false)
        .await
        .expect("link start is accepted");

    for expected in [LinkEvent::Connected, LinkEvent::Disconnected] {
        assert_eq!(
            timeout(Duration::from_secs(2), events.recv())
                .await
                .expect("link event arrives")
                .expect("event receiver remains active"),
            expected
        );
    }
}

#[derive(Deserialize)]
struct ErrorCatalog {
    errors: Vec<ErrorContract>,
}

#[derive(Deserialize)]
struct ErrorContract {
    code: String,
    source: String,
    severity: String,
    recovery: String,
    buffer_seconds: Option<u64>,
    backoff_start_ms: Option<u64>,
    backoff_ceiling_ms: Option<u64>,
    ui_message_key: String,
}

#[test]
fn emitted_capture_and_link_errors_have_canonical_recovery_contracts() {
    let catalog: ErrorCatalog =
        serde_yaml::from_str(include_str!("../../../contracts/errors.yaml"))
            .expect("error catalog parses");
    let errors = catalog
        .errors
        .into_iter()
        .map(|error| (error.code.clone(), error))
        .collect::<HashMap<_, _>>();

    let permission = &errors["CAPTURE_PERMISSION_DENIED"];
    assert_eq!(permission.source, "rust");
    assert_eq!(permission.severity, "fatal-capture");
    assert_eq!(permission.recovery, "open_os_settings");
    assert_eq!(permission.ui_message_key, "err.capture_permission_denied");

    let device = &errors["CAPTURE_DEVICE_LOST"];
    assert_eq!(device.source, "rust");
    assert_eq!(device.severity, "retryable");
    assert_eq!(device.recovery, "rebuild_affected_source");
    assert_eq!(device.ui_message_key, "err.capture_device_lost");

    let disconnected = &errors["LINK_DISCONNECTED"];
    assert_eq!(disconnected.source, "rust");
    assert_eq!(disconnected.severity, "retryable");
    assert_eq!(disconnected.recovery, "fresh_start_flush_buffer");
    assert_eq!(disconnected.buffer_seconds, Some(3));
    assert_eq!(disconnected.backoff_start_ms, Some(500));
    assert_eq!(disconnected.backoff_ceiling_ms, Some(10_000));
    assert_eq!(disconnected.ui_message_key, "err.link_disconnected");
}
