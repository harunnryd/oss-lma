use std::fs;
use std::path::PathBuf;

fn frontend_dist() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("tauri.conf.json")).expect("tauri config exists"),
    )
    .expect("tauri config is valid json");
    let frontend_dist = config["build"]["frontendDist"]
        .as_str()
        .expect("frontendDist is configured");
    root.join(frontend_dist)
}

#[test]
fn main_window_can_subscribe_to_native_events() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let capability: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("capabilities/main.json")).expect("main capability exists"),
    )
    .expect("main capability is valid json");
    let permissions = capability["permissions"]
        .as_array()
        .expect("permissions are configured");

    for required in ["core:event:allow-listen", "core:event:allow-unlisten"] {
        assert!(
            permissions.iter().any(|permission| permission == required),
            "main capability missing `{required}`"
        );
    }
}

macro_rules! skip_if_missing {
    ($path:expr) => {
        if !$path.exists() {
            eprintln!(
                "skip: {} not found (run `npm --prefix src run build` first)",
                $path.display()
            );
            return;
        }
    };
}

#[test]
fn tauri_frontend_dist_contains_index_html() {
    let index_path = frontend_dist().join("index.html");
    skip_if_missing!(index_path);
    assert!(index_path.is_file(), "index.html missing in frontendDist");
}

#[test]
fn tauri_frontend_dist_contains_assets_directory() {
    let assets = frontend_dist().join("assets");
    skip_if_missing!(assets);
    let entries: Vec<_> = fs::read_dir(&assets)
        .expect("assets is a directory")
        .filter_map(|e| e.ok())
        .collect();
    assert!(!entries.is_empty(), "assets/ directory must not be empty");

    let has_js = entries
        .iter()
        .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("js"));
    let has_css = entries
        .iter()
        .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("css"));
    assert!(has_js, "expected a hashed .js bundle in assets/");
    assert!(has_css, "expected a hashed .css bundle in assets/");
}

#[test]
fn frontend_bundle_contains_required_wire_protocol_strings() {
    let assets = frontend_dist().join("assets");
    skip_if_missing!(assets);

    let bundle = fs::read_dir(&assets)
        .expect("assets is a directory")
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().and_then(|s| s.to_str()) == Some("js"))
        .expect("a hashed .js bundle exists")
        .path();

    let bundle_text = fs::read_to_string(&bundle).expect("bundle readable");

    for required in [
        "ADD_TRANSCRIPT_SEGMENT",
        "DELETE_TRANSCRIPT_SEGMENT",
        "ERROR",
        "START",
    ] {
        assert!(
            bundle_text.contains(required),
            "bundle missing wire event `{required}`"
        );
    }

    for code in [
        "STT_PROVIDER_AUTH",
        "STT_STREAM_RESET",
        "LINK_DISCONNECTED",
        "CAPTURE_DEVICE_LOST",
        "CAPTURE_PERMISSION_DENIED",
        "VP_CONTAINER_FAILED",
        "VP_MANUAL_ACTION_REQUIRED",
        "AGENT_TOOL_FAILURE",
        "RAG_EMBEDDING_UNAVAILABLE",
        "DB_WRITE_CONFLICT",
        "SIDECAR_UNAVAILABLE",
        "PORT_BIND_FAILED",
    ] {
        assert!(
            bundle_text.contains(code),
            "bundle missing recovery code `{code}`"
        );
    }

    for channel in ["meeting-event", "capture-status"] {
        assert!(
            bundle_text.contains(channel),
            "bundle missing channel `{channel}`"
        );
    }
}

#[test]
fn frontend_bundle_does_not_leak_secrets_or_endpoint_details() {
    let assets = frontend_dist().join("assets");
    skip_if_missing!(assets);

    let bundle = fs::read_dir(&assets)
        .expect("assets is a directory")
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().and_then(|s| s.to_str()) == Some("js"))
        .expect("a hashed .js bundle exists")
        .path();

    let bundle_text = fs::read_to_string(&bundle).expect("bundle readable");

    for forbidden in [
        "sk-proj-",
        "DEEPGRAM_API_KEY=",
        "OPENAI_API_KEY=",
        "provider-secret",
        "port: 8765",
    ] {
        assert!(
            !bundle_text.contains(forbidden),
            "frontend bundle must not contain `{forbidden}`"
        );
    }
}
