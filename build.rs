//! Embed the skill's data files (prompt templates, model data, reference docs, SKILL.md) in the binary, so that a
//! `council` installed with `cargo install` works with no skill folder beside it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md" | "yaml" | "yml" | "json")
        ) {
            out.push(path);
        }
    }
}

fn main() {
    let root =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let skill = root.join("skills").join("council-loop");
    let mut files = Vec::new();
    for sub in ["templates", "models", "reference"] {
        walk(&skill.join(sub), &mut files);
        println!("cargo:rerun-if-changed=skills/council-loop/{sub}");
    }
    files.push(skill.join("SKILL.md"));
    println!("cargo:rerun-if-changed=skills/council-loop/SKILL.md");
    println!("cargo:rerun-if-changed=build.rs");

    let mut code = String::from("/// Every embedded file: its path inside the skill folder, and its text.\npub static FILES: &[(&str, &str)] = &[\n");
    for file in files.iter().filter(|f| f.is_file()) {
        let rel: Vec<String> = file
            .strip_prefix(&skill)
            .expect("the file is inside the skill folder")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let _ = writeln!(
            code,
            "    ({:?}, include_str!({:?})),",
            rel.join("/"),
            file.display().to_string()
        );
    }
    code.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("assets.rs");
    fs::write(out, code).expect("the build folder is writable");
}
