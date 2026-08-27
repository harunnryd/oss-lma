use std::fs;

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
