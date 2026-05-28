//! Sync embedded assets from the repo root into `embedded/` so they're
//! reachable from inside the packaged crate (cargo publish strips parent
//! directories; `include_str!("../../...")` no longer resolves).
//!
//! In a fresh clone the `embedded/` copies are committed, so this script
//! is a no-op when the root sources are absent (e.g. when building from
//! a published crate). When the root sources exist (dev), they win and
//! get copied in before the compile reads them.

use std::path::{Path, PathBuf};

const ASSETS: &[(&str, &str)] = &[
    ("../../data/talon.txt", "talon.txt"),
    ("../../skill/SKILL.md", "SKILL.md"),
];

fn sync(src: &Path, dst: &Path) {
    let Ok(src_bytes) = std::fs::read(src) else {
        return;
    };
    let needs_write = std::fs::read(dst).map_or(true, |existing| existing != src_bytes);
    if needs_write {
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(dst, src_bytes);
    }
}

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let embedded = manifest_dir.join("embedded");

    for (src_rel, dst_name) in ASSETS {
        let src = manifest_dir.join(src_rel);
        let dst = embedded.join(dst_name);
        if src.exists() {
            sync(&src, &dst);
            println!("cargo:rerun-if-changed={}", src.display());
        }
    }
}
