//! The licences of the crates in the binary.
//!
//! `THIRD-PARTY-NOTICES.md` lists every crate the `council` binary is built from, on any system,
//! with its licence, and holds the licence texts those crates ship. This test fails when the list
//! no longer matches `Cargo.lock`: a dependency was added, removed or updated and the notices were
//! not made again. Make them again, then review the diff like any other change:
//!
//! ```text
//! COUNCIL_ACCEPT=1 cargo test --test notices
//! ```

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// One crate the binary is built from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Crate {
    name: String,
    version: String,
    licence: String,
    source: String,
    folder: PathBuf,
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every crate reachable from this one through dependencies that are compiled into a build (not
/// the ones only tests use), for every system the crate can be built for.
fn crates() -> Vec<Crate> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(repo())
        .output()
        .expect("cargo runs");
    assert!(
        out.status.success(),
        "cargo metadata failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: Value = serde_json::from_slice(&out.stdout).expect("cargo metadata prints JSON");
    let packages: BTreeMap<&str, &Value> = meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let nodes: BTreeMap<&str, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();
    let root = meta["resolve"]["root"].as_str().expect("a root package");
    let (mut seen, mut stack) = (BTreeSet::new(), vec![root]);
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        for dep in nodes[id]["deps"].as_array().into_iter().flatten() {
            let built_in = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].is_null());
            if let (true, Some(pkg)) = (built_in, dep["pkg"].as_str()) {
                stack.push(pkg);
            }
        }
    }
    seen.remove(root);
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let mut found: Vec<Crate> = seen
        .iter()
        .map(|id| {
            let p = packages[id];
            Crate {
                name: text(&p["name"]),
                version: text(&p["version"]),
                licence: text(&p["license"]).replace('/', " OR "),
                source: text(&p["repository"]),
                folder: Path::new(p["manifest_path"].as_str().unwrap_or_default())
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default(),
            }
        })
        .collect();
    found.sort();
    found
}

fn row(c: &Crate) -> String {
    format!(
        "| {} | {} | {} | {} |",
        c.name, c.version, c.licence, c.source
    )
}

/// The licence files a crate ships: (file name, text with plain line ends).
fn licence_files(folder: &Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let upper = name.to_uppercase();
            let wanted = [
                "LICENSE",
                "LICENCE",
                "COPYING",
                "UNLICENSE",
                "NOTICE",
                "COPYRIGHT",
            ]
            .iter()
            .any(|start| upper.starts_with(start));
            let meta = entry.metadata().ok()?;
            if !wanted || !meta.is_file() || meta.len() > 200_000 {
                return None;
            }
            let raw = std::fs::read(entry.path()).ok()?;
            let text: Vec<String> = String::from_utf8_lossy(&raw)
                .replace("\r\n", "\n")
                .lines()
                .map(|l| l.trim_end().to_string())
                .collect();
            Some((name, text.join("\n").trim().to_string()))
        })
        .collect();
    files.sort();
    files
}

fn written(crates: &[Crate]) -> String {
    let mut out = String::from(
        "# Third-party notices\n\n\
         The `council` binary is built from the crates below (for all the systems it can be built for).\n\
         Each row names the licence its authors offer; `OR` means a choice. The licence texts those\n\
         crates ship follow the table.\n\n\
         This file is made by a test, from `Cargo.lock` and the crates' own files. Do not edit it by hand:\n\
         `COUNCIL_ACCEPT=1 cargo test --test notices` makes it again, and `cargo test` fails when it is\n\
         out of date.\n\n\
         The prompt templates, reference pages and model data in the binary are this project's own\n\
         (MIT, see `LICENSE`). Model data © Artificial Analysis, used with attribution as their API terms\n\
         require.\n\n\
         | Crate | Version | Licence | Source |\n| --- | --- | --- | --- |\n",
    );
    for c in crates {
        out.push_str(&row(c));
        out.push('\n');
    }
    // the same text is printed once, with every crate that ships it
    let mut texts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in crates {
        for (file, text) in licence_files(&c.folder) {
            if !text.is_empty() {
                texts
                    .entry(text)
                    .or_default()
                    .push(format!("{} {} ({file})", c.name, c.version));
            }
        }
    }
    let mut blocks: Vec<(Vec<String>, String)> = texts
        .into_iter()
        .map(|(text, users)| (users, text))
        .collect();
    blocks.sort();
    out.push_str("\n## Licence texts\n");
    for (n, (users, text)) in blocks.iter().enumerate() {
        out.push_str(&format!(
            "\n### {}. {}\n\n~~~~text\n{}\n~~~~\n",
            n + 1,
            users.join(", "),
            text.replace("~~~~", "~~~ ~")
        ));
    }
    out
}

#[test]
fn the_notices_list_every_crate_in_the_binary() {
    let crates = crates();
    assert!(
        crates.len() > 50,
        "only {} crates found: is Cargo.lock there?",
        crates.len()
    );
    let file = repo().join("THIRD-PARTY-NOTICES.md");
    if std::env::var("COUNCIL_ACCEPT").is_ok_and(|v| v == "1") {
        std::fs::write(&file, written(&crates)).expect("the notices can be written");
        println!("wrote {} ({} crates)", file.display(), crates.len());
        return;
    }
    let held = std::fs::read_to_string(&file)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    let listed: BTreeSet<&str> = held
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Crate ") && !l.starts_with("| ---"))
        .collect();
    let wanted: Vec<String> = crates.iter().map(row).collect();
    let wanted: BTreeSet<&str> = wanted.iter().map(String::as_str).collect();
    let missing: Vec<&&str> = wanted.difference(&listed).collect();
    let stale: Vec<&&str> = listed.difference(&wanted).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "THIRD-PARTY-NOTICES.md is out of date ({} crate(s) not listed, {} listed that are no longer used).\n\
         Make it again and review the diff:  COUNCIL_ACCEPT=1 cargo test --test notices\n\
         not listed: {missing:?}\nno longer used: {stale:?}",
        missing.len(),
        stale.len()
    );
    // a crate that ships no licence file would be missing from the texts
    for c in &crates {
        assert!(
            !licence_files(&c.folder).is_empty(),
            "{} {} ships no licence file: add its text by hand and say so here",
            c.name,
            c.version
        );
    }
}
