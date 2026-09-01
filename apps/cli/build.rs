use std::path::PathBuf;

// rust-embed fails to compile if its folder is missing, and apps/dashboard/dist
// is gitignored — a fresh clone has no dist until someone runs the web build.
// Create the directory so `cargo build` alone still works; the binary then
// serves the build-me notice instead of a dashboard.
fn main() {
    let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/dashboard/dist");
    if !dist.exists() {
        let _ = std::fs::create_dir_all(&dist);
    }
    println!("cargo:rerun-if-changed=../../apps/dashboard/dist");
}
