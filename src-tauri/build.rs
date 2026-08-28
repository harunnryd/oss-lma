fn main() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(manifest_dir.join("../target/sidecar/sidecar"))
        .expect("sidecar resource directory must be creatable");
    println!("cargo:rerun-if-changed=../src/dist/index.html");
    tauri_build::build()
}
