//! Where this tool writes, what it reads and what it prints.
//!
//! ELI5: a project folder can come from anyone. It can hold links that point at your private
//! files, a settings file that names a folder far away, text that drives a terminal, or a file that
//! never ends. This module is the one door every write, every risky read and every printed line
//! goes through, so a repository's author never gets to choose where this tool writes, what it
//! sends to a model, or what your terminal does.
//!
//! The rules (the threat model is in SECURITY.md):
//!
//! * **Writes** land only inside the project, your settings folder (`~/.council`), the private
//!   folder (`~/.council-private`), a memory folder that your own settings or the environment name,
//!   or a path you typed on the command line. Never through a symbolic link.
//! * **Reads of the project's own state** take plain files only: not a link, not a device, not a
//!   pipe, and not more than a stated size.
//! * **Text from files and models** loses its control characters before it is printed or put in a
//!   packet.
//! * **git** is asked to look, never to run what the repository's settings name.

use crate::core::{self, fail};
use anyhow::{Context, Result};
use serde_json::Value;
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Bytes of one seat output (or of a map to install). A larger file is refused unread.
pub const MAX_OUTPUT: u64 = 2_000_000;
/// Bytes of one locked file.
pub const MAX_LOCKED: u64 = 64_000_000;
/// Bytes of any other file this tool reads on its own (settings, run state, notes, prompts).
pub const MAX_READ: u64 = 64_000_000;

/// Folders this tool may write under besides the project and your own folders: a memory folder that
/// your settings or the environment name, a folder `council refresh` keeps model data in.
static EXTRA_ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

// ----------------------------------------------------------------------------- text

/// A character that could steer a terminal or hide text from a reader: the C0 and C1 controls
/// (escape sequences start with one), Unicode's direction overrides and isolates, and the "tag"
/// letters that are invisible to people and readable by models.
fn odd(c: char) -> bool {
    matches!(c,
        '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}'..='\u{9f}'
        | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{e0000}'..='\u{e007f}')
}

/// Text that is safe to print or to put in a packet: every [`odd`] character becomes U+FFFD and a
/// carriage return (which could overwrite the line it is printed on) is dropped. Tabs and line
/// ends stay.
pub fn shown(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|c| c == '\r' || odd(c)) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter(|c| *c != '\r')
            .map(|c| if odd(c) { '\u{fffd}' } else { c })
            .collect(),
    )
}

/// Does a name (of a file, say) hold a character that [`shown`] would replace?
pub fn odd_name(name: &str) -> bool {
    name.chars().any(|c| c == '\r' || c == '\n' || odd(c))
}

/// A parsed JSON or YAML value with every string passed through [`shown`]: what a seat or a file
/// said, without the characters that could steer a terminal or hide text from a reader.
pub fn cleaned(value: Value) -> Value {
    match value {
        Value::String(s) => match shown(&s) {
            Cow::Borrowed(_) => Value::String(s),
            Cow::Owned(clean) => Value::String(clean),
        },
        Value::Array(list) => Value::Array(list.into_iter().map(cleaned).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (shown(&k).into_owned(), cleaned(v)))
                .collect(),
        ),
        other => other,
    }
}

/// A document without the code fence a model sometimes puts around the whole of it. One pass over
/// the text: no pattern, so no input can make it slow.
pub fn unfence(raw: &str) -> String {
    let text = crate::py::strip(raw);
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let rest = rest.trim_end_matches(crate::py::is_space);
    if matches!(crate::py::strip(first), "```" | "```markdown" | "```md") {
        if let Some(body) = rest.strip_suffix("```") {
            if let Some(body) = body.strip_suffix('\n') {
                return crate::py::strip(body).to_string();
            }
        }
    }
    text.to_string()
}

// ----------------------------------------------------------------------------- reads

/// The size of `path` when it is a plain file of at most `limit` bytes: not a symbolic link, not a
/// folder, not a device or a pipe (those never stop giving bytes).
pub fn plain_file(path: &Path, limit: u64) -> Option<u64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    (meta.file_type().is_file() && meta.len() <= limit).then_some(meta.len())
}

/// Is `path` a symbolic link (or, on Windows, a junction)?
pub fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Read a file that is part of the project's own state (run files, settings, notes, the map): a
/// plain file only. A link is not followed: its target could be any file on the machine, and what
/// this tool reads can end up in a packet that leaves it.
pub fn read_own(path: &Path) -> Result<String> {
    read_capped(path, MAX_READ)
}

/// [`read_own`] with a size limit of the caller's choosing.
pub fn read_capped(path: &Path, limit: u64) -> Result<String> {
    if is_link(path) {
        return fail(format!(
            "refusing to read {}: it is a symbolic link",
            path.display()
        ));
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_file() => {
            return fail(format!(
                "refusing to read {}: it is not a plain file",
                path.display()
            ))
        }
        Ok(meta) if meta.len() > limit => {
            return fail(format!(
                "refusing to read {}: it is {} bytes; the limit is {}",
                path.display(),
                crate::py::commas(meta.len() as i64),
                crate::py::commas(limit as i64)
            ))
        }
        _ => {}
    }
    core::read_text(path).with_context(|| format!("cannot read {}", path.display()))
}

// ----------------------------------------------------------------------------- where this tool may write

/// Name a folder this tool may write under: a memory folder that your own settings or the
/// environment name, the folder model data is refreshed into. Never called with a value read from
/// a project.
pub fn allow_root(path: &Path) {
    if let Ok(mut roots) = EXTRA_ROOTS.lock() {
        if !roots.iter().any(|r| r == path) {
            roots.push(path.to_path_buf());
        }
    }
}

/// The folder that holds what must stay away from the seats: `~/.council-private`, or what
/// `COUNCIL_PRIVATE_DIR` names.
pub fn private_dir() -> PathBuf {
    match std::env::var_os("COUNCIL_PRIVATE_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => crate::py::plain(dir),
        None => core::home_dir().join(".council-private"),
    }
}

/// Follow every symbolic link in `path`, component by component, as far as the path exists. What
/// does not exist yet is kept as written (`..` steps back).
fn follow(path: &Path, hops: &mut u32) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Prefix(_) | Component::RootDir => out.push(part.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => {
                out.push(name);
                if *hops < 40 {
                    if let Ok(target) = std::fs::read_link(&out) {
                        *hops += 1;
                        out.pop();
                        out = follow(&out.join(target), hops);
                    }
                }
            }
        }
    }
    out
}

/// The part of `path` that exists, spelled the way the file system spells it (letter case on
/// Windows and macOS, short names on Windows), with the rest appended as written.
fn canonical_prefix(path: &Path) -> PathBuf {
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut head = path.to_path_buf();
    loop {
        if let Ok(real) = dunce::canonicalize(&head) {
            return tail.iter().rev().fold(real, |acc, name| acc.join(name));
        }
        match (
            head.parent().map(Path::to_path_buf),
            head.file_name().map(std::ffi::OsStr::to_os_string),
        ) {
            (Some(parent), Some(name)) => {
                tail.push(name);
                head = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The path with every symbolic link followed, whether or not all of it exists yet. A link whose
/// target does not exist is followed too: writing "to the link" would create that target.
pub fn real(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    canonical_prefix(&follow(&absolute, &mut 0))
}

/// Is `path`, with links followed, inside `root` (or `root` itself)?
pub fn within(path: &Path, root: &Path) -> bool {
    real(path).starts_with(real(root))
}

/// The folders this tool may write under right now.
fn roots() -> Vec<PathBuf> {
    let mut all = vec![core::project_root(), private_dir()];
    if let Some(own) = core::user_config_path().parent() {
        all.push(own.to_path_buf());
    }
    if let Ok(extra) = EXTRA_ROOTS.lock() {
        all.extend(extra.iter().cloned());
    }
    all
}

/// Refuse a path this tool must not write: a symbolic link, or a place that, with links followed,
/// lies outside the project, your settings folder, the private folder and the memory your own
/// settings name. A repository can hold links that point anywhere; following one would let its
/// author choose where this tool writes.
pub fn guard(path: &Path, folder: bool) -> Result<PathBuf> {
    let p = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    if is_link(&p) {
        return fail(format!(
            "refusing to write {}: it is a symbolic link",
            p.display()
        ));
    }
    let place = if folder {
        p.as_path()
    } else {
        p.parent().unwrap_or(&p)
    };
    let place = real(place);
    if !roots().iter().any(|r| place.starts_with(real(r))) {
        crate::diag!(
            Warn,
            "safe",
            "a write was refused: {} leads to {}",
            p.display(),
            place.display()
        );
        return fail(format!(
            "refusing to write {}: with links followed it lies outside the project",
            p.display()
        ));
    }
    Ok(p)
}

/// Open a file for writing from its start, without following a link in its last part.
fn create(path: &Path, private: bool) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
        if private {
            options.mode(0o600);
        }
    }
    #[cfg(not(unix))]
    let _ = private;
    options.open(path)
}

fn put(path: &Path, text: &str, private: bool) -> Result<()> {
    use std::io::Write as _;
    create(path, private)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .with_context(|| format!("cannot write {}", path.display()))
}

/// The one way this tool writes a file of its own: UTF-8 exactly as given, the folders above it
/// created, and only where it should (see [`guard`]).
pub fn write_text(path: &Path, text: &str) -> Result<()> {
    let p = guard(path, false)?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    put(&p, text, false)
}

/// Add one line to the end of a file of this tool's own (a run's event log). Checked like any
/// other write; the file is created when it is not there.
pub fn append_line(path: &Path, line: &str) -> Result<()> {
    use std::io::Write as _;
    let p = guard(path, false)?;
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(&p)
        .and_then(|mut file| file.write_all(format!("{line}\n").as_bytes()))
        .with_context(|| format!("cannot write {}", p.display()))
}

/// Write a file at a path the person named on the command line (`--out`). It is theirs to choose:
/// the place is not checked and a link is followed, as any program writing to a named path does.
pub fn write_named(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))
}

/// Create a folder (and its parents) where this tool may write.
pub fn make_dir(path: &Path) -> Result<PathBuf> {
    let p = guard(path, true)?;
    std::fs::create_dir_all(&p).with_context(|| format!("cannot create {}", p.display()))?;
    Ok(p)
}

/// Create a folder that must not exist yet (its parents are created as needed). `Ok(false)` when
/// it is already there.
pub fn make_new_dir(path: &Path) -> Result<bool> {
    let p = guard(path, true)?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    match std::fs::create_dir(&p) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err).with_context(|| format!("cannot create {}", p.display())),
    }
}

/// Move a file in this tool's folders to another name. Both ends must lie where this tool may
/// write. A link at either end is not followed: a rename moves or replaces the link itself.
pub fn rename(from: &Path, to: &Path) -> Result<()> {
    for end in [from, to] {
        let place = real(end.parent().unwrap_or(end));
        if !roots().iter().any(|r| place.starts_with(real(r))) {
            return fail(format!(
                "refusing to move {}: with links followed it lies outside the project",
                end.display()
            ));
        }
    }
    std::fs::rename(from, to)
        .with_context(|| format!("cannot move {} to {}", from.display(), to.display()))
}

/// Make a folder only its owner may enter (a no-op on Windows, where the folder's own permissions
/// apply).
pub fn owner_only(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Write a file only its owner may read: who wrote which card must stay away from the seats and
/// from the other accounts on the machine. On Windows the folder's own permissions apply.
pub fn private_file(path: &Path, text: &str) -> Result<()> {
    let p = guard(path, false)?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
        if real(parent).starts_with(real(&private_dir())) {
            owner_only(parent);
        }
    }
    put(&p, text, true)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

// ----------------------------------------------------------------------------- other programs

/// A git command for a repository this tool did not create. The repository's own settings can name
/// programs for git to run (`core.fsmonitor`, for one); those are switched off, and git is told
/// not to take locks, so that looking at a repository never runs or changes anything in it.
pub fn git(root: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.args([
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=",
        "--no-optional-locks",
        "-C",
    ])
    .arg(root)
    .stdin(std::process::Stdio::null());
    cmd
}

/// `None` when `rx` is a pattern this tool will run from a project's settings file, else why not.
/// The file comes with the repository, so only patterns that match in time proportional to the
/// text are taken: no look-around and no back-references, and not more than 200 characters.
pub fn simple_pattern(rx: &Value) -> Option<String> {
    let Some(rx) = rx.as_str() else {
        return Some("it is not text".to_string());
    };
    if rx.chars().count() > 200 {
        return Some("it is longer than 200 characters".to_string());
    }
    match project_pattern(rx) {
        Ok(_) => None,
        Err(_) => Some(
            "it is not a pattern this tool runs (no look-around, no back-references)".to_string(),
        ),
    }
}

/// Compile a pattern from a settings file: letter case ignored, a bounded size, linear-time matching.
pub fn project_pattern(rx: &str) -> std::result::Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(rx)
        .case_insensitive(true)
        .size_limit(1 << 20)
        .build()
}

/// An argument that will be handed to another program as an option's value: it must not look like
/// an option itself, and must be one plain line.
pub fn plain_arg(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-') && !odd_name(value) && value.len() <= 200
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn control_characters_are_replaced_and_plain_text_is_left_alone() {
        assert!(matches!(
            shown("plain text\twith a tab\n"),
            Cow::Borrowed(_)
        ));
        assert_eq!(shown("a\x1b[31mred\x07"), "a\u{fffd}[31mred\u{fffd}");
        assert_eq!(shown("line\r\nnext\rover"), "line\nnextover");
        assert_eq!(
            shown("x\u{202e}y\u{2067}z\u{9b}"),
            "x\u{fffd}y\u{fffd}z\u{fffd}"
        );
        assert_eq!(shown("tag\u{e0041}\u{e007f}"), "tag\u{fffd}\u{fffd}");
        assert_eq!(shown("é ñ 日本 \u{200d}"), "é ñ 日本 \u{200d}");
        assert!(odd_name("a\nb") && odd_name("a\x1bb") && !odd_name("a b.py"));
        assert_eq!(
            cleaned(json!({"k\x1b": ["v\x00", 1, {"n": "ok"}]})),
            json!({"k\u{fffd}": ["v\u{fffd}", 1, {"n": "ok"}]})
        );
    }

    #[test]
    fn a_fence_around_the_whole_document_is_removed() {
        assert_eq!(
            unfence("```markdown\n# Map\n\ntext\n```\n"),
            "# Map\n\ntext"
        );
        assert_eq!(unfence("  ```\nbody\n```  "), "body");
        assert_eq!(unfence("```md\nbody\n```"), "body");
        assert_eq!(unfence("# Map\n```\ncode\n```"), "# Map\n```\ncode\n```");
        assert_eq!(unfence("```python\nx = 1\n```"), "```python\nx = 1\n```");
        assert_eq!(unfence("```\n```"), "```\n```");
        assert_eq!(unfence(""), "");
    }

    #[test]
    fn patterns_from_a_settings_file_must_be_linear() {
        assert_eq!(simple_pattern(&json!(r"\bbilling\b|\binvoice\w*")), None);
        assert_eq!(simple_pattern(&json!("(a+)+$")), None); // linear in this engine, whatever it looks like
        assert!(simple_pattern(&json!(r"(?<!x)y")).is_some());
        assert!(simple_pattern(&json!(r"(a)\1")).is_some());
        assert!(simple_pattern(&json!("(")).is_some());
        assert!(simple_pattern(&json!(7)).is_some());
        assert!(simple_pattern(&json!("a".repeat(201))).is_some());
        assert!(simple_pattern(&json!("a{1000}{1000}{1000}")).is_some());
    }

    #[test]
    fn option_values_for_other_programs_are_plain() {
        assert!(plain_arg("gpt-6.1-sol") && plain_arg("anthropic/claude-fable-5.2"));
        assert!(
            !plain_arg("--dangerously-skip-permissions") && !plain_arg("") && !plain_arg("a\nb")
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_are_followed_even_when_their_target_does_not_exist() {
        let base = std::env::temp_dir().join(format!("council-safe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("project/sub")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        let base = dunce::canonicalize(&base).unwrap();
        std::os::unix::fs::symlink(base.join("outside/new.txt"), base.join("project/dangling"))
            .unwrap();
        std::os::unix::fs::symlink(base.join("outside"), base.join("project/dir")).unwrap();
        assert_eq!(
            real(&base.join("project/dangling")),
            base.join("outside/new.txt")
        );
        assert_eq!(
            real(&base.join("project/dir/x/y")),
            base.join("outside/x/y")
        );
        assert_eq!(
            real(&base.join("project/sub/../nope/z")),
            base.join("project/nope/z")
        );
        assert!(within(&base.join("project/sub/a"), &base.join("project")));
        assert!(!within(&base.join("project/dir/a"), &base.join("project")));
        assert!(!within(&base.join("project-2"), &base.join("project")));
        assert!(is_link(&base.join("project/dangling")));
        assert_eq!(plain_file(&base.join("project/dangling"), 10), None);
        assert!(read_own(&base.join("project/dir")).is_err());
        // this tool's own writes never go through a link, whether its target exists or not
        allow_root(&base.join("project"));
        let refused = write_text(&base.join("project/dangling"), "x").unwrap_err();
        assert!(
            refused.to_string().contains("it is a symbolic link"),
            "{refused}"
        );
        let refused = write_text(&base.join("project/dir/new.txt"), "x").unwrap_err();
        assert!(
            refused.to_string().contains("outside the project"),
            "{refused}"
        );
        assert!(make_dir(&base.join("project/dir/deeper")).is_err());
        assert!(!base.join("outside/new.txt").exists() && !base.join("outside/deeper").exists());
        write_text(&base.join("project/sub/made/here.txt"), "x").unwrap();
        assert_eq!(
            std::fs::read_to_string(base.join("project/sub/made/here.txt")).unwrap(),
            "x"
        );
        // a path the person named is theirs to choose
        write_named(&base.join("outside/named.txt"), "y").unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }
}
