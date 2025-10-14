use std::env;
use std::path::{Path, PathBuf};

pub fn normalize_root<P: AsRef<Path>>(p: P) -> std::io::Result<PathBuf> {
    let pb = p.as_ref();
    if pb.is_absolute() { Ok(pb.to_path_buf()) } else { std::fs::canonicalize(pb) }
}

fn xdg_state_home() -> PathBuf {
    if let Ok(dir) = env::var("XDG_STATE_HOME") {
        return PathBuf::from(dir);
    }
    if let Ok(home) = env::var("HOME") {
        return Path::new(&home).join(".local/state");
    }
    PathBuf::from(".oxproc-state")
}

pub fn project_id<P: AsRef<Path>>(root: P) -> String {
    let canonical = std::fs::canonicalize(root.as_ref()).unwrap_or_else(|_| root.as_ref().to_path_buf());
    let mut hasher = blake3::Hasher::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    hash.to_hex()[..12].to_string()
}

pub fn state_dir_for_project<P: AsRef<Path>>(root: P) -> PathBuf {
    let id = project_id(root.as_ref());
    xdg_state_home().join("oxproc").join(id)
}
