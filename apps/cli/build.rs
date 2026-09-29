use std::path::PathBuf;

// rust-embed fails to compile if its folder is missing, and both
// apps/dashboard/dist and apps/home/dist are gitignored — a fresh clone has
// neither until the matching app build runs. Create both so `cargo build`
// alone still works: the binary then serves the build-me notice at `/` in
// place of a dashboard, and `archie home` refuses to start rather than
// serving a blank page. Two rust-embed folders, two directories.
fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for rel in ["../../apps/dashboard/dist", "../../apps/home/dist"] {
        let dist = manifest.join(rel);
        if !dist.exists() {
            let _ = std::fs::create_dir_all(&dist);
        }
        println!("cargo:rerun-if-changed={rel}");
    }
}
