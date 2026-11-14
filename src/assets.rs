//! The skill's data files: prompt templates, model data and SKILL.md.
//!
//! They are compiled into the binary, so `council` works on its own. When a skill folder is found
//! on disk (named by `COUNCIL_SKILL_DIR`, or beside the executable as in a checkout of the
//! repository), its files win: that is how a team changes a template or refreshes the model data
//! without rebuilding.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// The crate's version, as `Cargo.toml` states it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn is_skill(dir: &Path) -> bool {
    dir.join("templates").is_dir() && dir.join("models").is_dir()
}

fn find_skill_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("COUNCIL_SKILL_DIR").map(crate::py::plain) {
        if is_skill(&dir) {
            return Some(dir);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    exe.ancestors()
        .skip(1)
        .take(4)
        .map(|dir| dir.join("skills").join("council-loop"))
        .find(|dir| is_skill(dir))
}

/// The skill folder on disk, when there is one.
pub fn skill_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(find_skill_dir).as_deref()
}

/// The text of a skill file such as `templates/packets/vote.md`: from disk when a skill folder is
/// present and has the file, otherwise the copy compiled into the binary.
pub fn read(rel: &str) -> Option<String> {
    if let Some(dir) = skill_dir() {
        if let Ok(text) = crate::core::read_text(&dir.join(rel)) {
            return Some(text);
        }
    }
    FILES
        .iter()
        .find(|(name, _)| *name == rel)
        .map(|(_, text)| (*text).to_string())
}

/// Where a skill file lives, for messages: the path on disk, or a note that it is built in.
pub fn shown(rel: &str) -> String {
    match skill_dir() {
        Some(dir) if dir.join(rel).exists() => dir.join(rel).display().to_string(),
        _ => format!("{rel} (built into council {VERSION})"),
    }
}

/// Where `council refresh` writes model data: the skill folder on disk, or your own settings folder.
pub fn models_dir() -> PathBuf {
    match skill_dir() {
        Some(dir) => dir.join("models"),
        None => crate::core::user_config_path()
            .parent()
            .map(|p| p.join("models"))
            .unwrap_or_else(|| PathBuf::from("models")),
    }
}

/// Model data (`policy.yaml`, `lock.yaml`). A skill folder on disk is the source of truth: a file
/// missing there is missing (the fix is `council refresh`, not older data from inside the binary).
/// With no skill folder, a refreshed copy in your settings folder wins over the built-in data.
pub fn read_model(name: &str) -> Option<String> {
    if skill_dir().is_some() {
        return crate::core::read_text(&models_dir().join(name)).ok();
    }
    crate::core::read_text(&models_dir().join(name))
        .ok()
        .or_else(|| read(&format!("models/{name}")))
}
