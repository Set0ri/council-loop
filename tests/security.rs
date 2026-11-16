//! The attacks of the security review, kept as tests.
//!
//! Each test builds a hostile project (a repository can come from anyone), runs the real binary
//! in it, and checks that the attack fails: nothing is written outside the project, nothing of
//! the machine leaks into a packet, no program named by the repository runs, and nothing hangs.
//! SECURITY.md describes the threat model and what each of these stands for.

mod common;

use common::{has_git, Sandbox};
#[cfg(unix)]
use council_loop::demo::{script, Stage};
use std::time::Duration;

#[cfg(unix)]
fn link(target: impl AsRef<std::path::Path>, at: impl AsRef<std::path::Path>) {
    std::os::unix::fs::symlink(target, at).expect("a symbolic link");
}

/// A map that passes validation, built from the project's own skeleton.
#[cfg(unix)]
fn a_map(s: &Sandbox) -> String {
    assert_eq!(s.run(&["map"]).code, 0);
    let skeleton = s.proj.join(".council").join("map").join("skeleton.md");
    script::codebase_map(&format!(
        "skeleton made by a script: `{}`",
        skeleton.display()
    ))
    .expect("a map")
}

#[cfg(unix)]
#[test]
fn a_link_at_the_map_path_does_not_decide_where_the_map_is_written() {
    let s = Sandbox::new("map-link");
    s.write("app.py", "def main():\n    return 1\n");
    std::fs::create_dir_all(s.proj.join("docs")).unwrap();
    // the target does not exist yet: writing "to the link" would create it
    link(
        s.victim.join("authorized_keys"),
        s.proj.join("docs").join("ARCHITECTURE.md"),
    );
    let map = a_map(&s);
    s.write(
        "map.md",
        &format!("{map}\n\n```\nssh-ed25519 AAAAC3attackerkey attacker@evil\n```\n"),
    );
    let out = s.run(&["map", "--install", "map.md"]);
    assert_eq!(out.code, 2, "{}", out.all());
    assert!(out.err.contains("it is a symbolic link"), "{}", out.all());
    assert!(!s.victim.join("authorized_keys").exists());
}

#[cfg(unix)]
#[test]
fn links_inside_the_project_do_not_lead_writes_out_of_it() {
    // the memory folder is a link
    let s = Sandbox::new("memory-link");
    std::fs::create_dir_all(s.proj.join(".council")).unwrap();
    link(&s.victim, s.proj.join(".council").join("memory"));
    let out = s.run(&[
        "memory",
        "add",
        "--claim",
        "Queue A keeps order within one partition only",
        "--source",
        "https://example.org/a",
    ]);
    assert_eq!(out.code, 2, "{}", out.all());
    assert!(
        out.err
            .contains("leads out of the project through a symbolic link"),
        "{}",
        out.all()
    );
    assert_eq!(std::fs::read_dir(&s.victim).unwrap().count(), 0);

    // the whole .council folder is a link
    let s = Sandbox::new("council-link");
    std::fs::write(
        s.victim.join("config.yaml"),
        "precious: settings of another app\n",
    )
    .unwrap();
    link(&s.victim, s.proj.join(".council"));
    let out = s.run(&["memory", "setup", "--files", "--dir", "notes", "--project"]);
    assert_eq!(out.code, 2, "{}", out.all());
    assert_eq!(
        std::fs::read_to_string(s.victim.join("config.yaml")).unwrap(),
        "precious: settings of another app\n"
    );
    let out = s.run(&[
        "init",
        "Pick the queue technology for the billing pipeline",
        "--tier",
        "lite",
    ]);
    assert_ne!(out.code, 0, "{}", out.all());
    assert_eq!(
        std::fs::read_dir(&s.victim).unwrap().count(),
        1,
        "{}",
        out.all()
    );
}

#[test]
fn a_project_cannot_send_the_shared_memory_somewhere_else() {
    let s = Sandbox::new("memory-outside");
    let outside = s.victim.join("notes");
    s.write(
        ".council/config.yaml",
        &format!("memory:\n  dir: {}\n", outside.display()),
    );
    let out = s.run(&[
        "memory",
        "add",
        "--claim",
        "Queue A keeps order within one partition only",
        "--source",
        "https://example.org/a",
    ]);
    assert_eq!(out.code, 2, "{}", out.all());
    assert!(
        out.err
            .contains("A project may only name a folder inside itself"),
        "{}",
        out.all()
    );
    assert!(!outside.exists());
    // the same folder named by the person's own settings is theirs to choose
    let out = s.run_with(
        &[
            "memory",
            "add",
            "--claim",
            "Queue A keeps order within one partition only",
            "--source",
            "https://example.org/a",
        ],
        &[("COUNCIL_MEMORY_DIR", &outside.display().to_string())],
        None,
    );
    assert_eq!(out.code, 0, "{}", out.all());
    assert!(outside.join("research").is_dir());
    // and --project refuses to write such a setting
    let out = s.run(&[
        "memory",
        "setup",
        "--files",
        "--dir",
        &outside.display().to_string(),
        "--project",
    ]);
    assert_eq!(out.code, 2, "{}", out.all());
    assert!(
        out.err
            .contains("--project would point the shared memory outside the project"),
        "{}",
        out.all()
    );
}

#[test]
fn a_run_is_named_by_a_name_and_its_file_is_checked_before_it_is_believed() {
    let s = Sandbox::new("run-state");
    let runs = s.proj.join(".council").join("runs");
    std::fs::create_dir_all(s.victim.join("run")).unwrap();
    std::fs::write(
        s.victim.join("run").join("run.json"),
        r#"{"id":"x","phase":"brief","tier":"lite","loop":1,"seats":{},"ask":"a"}"#,
    )
    .unwrap();
    s.write(".council/runs/CURRENT", "../../../victim/run\n");
    let out = s.run(&["status"]);
    assert_eq!(out.code, 2);
    assert!(out.err.contains("is not a run id"), "{}", out.all());
    assert_eq!(s.run(&["status", "--run", "/etc"]).code, 2);
    // a run file that is not one, in every way the tool reads it
    let good = serde_json::json!({
        "id": "r1", "ask": "Pick the queue technology", "gate": "GATE tier=lite", "tier": "lite", "phase": "ideate",
        "calls": {}, "loops": [{}], "deviations": [], "seats": {"L1": {"role": "expert", "via": "cli:claude", "provider": "claude"}},
        "loop": 1, "max_loops": 1, "schedule": [3, 1], "shape": {}, "support": {}, "chair": {}, "commons": [],
        "memory": {"backend": "files", "dir": s.proj.join(".council").join("memory").display().to_string(), "ok": true}});
    let try_with = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut run = good.clone();
        change(&mut run);
        std::fs::create_dir_all(runs.join("r1")).unwrap();
        std::fs::write(runs.join("r1").join("run.json"), run.to_string()).unwrap();
        s.run(&["next", "--run", "r1"])
    };
    for (what, change) in [
        (
            "its id is not the name of its folder",
            (&|r: &mut serde_json::Value| r["id"] = "other".into())
                as &dyn Fn(&mut serde_json::Value),
        ),
        ("`seats` is missing or of the wrong kind", &|r| {
            r["seats"] = "none".into()
        }),
        ("its tier or its phase is not one this tool knows", &|r| {
            r["phase"] = "pwned".into()
        }),
        ("its loop counter does not fit its loops", &|r| {
            r["loop"] = 7.into()
        }),
        ("is not a seat", &|r| {
            r["seats"] = serde_json::json!({"../../x": {"role": "expert"}})
        }),
        // a model id is handed to another program as an option's value
        ("names a native_id that is not one", &|r| {
            r["seats"]["L1"]["native_id"] = "--dangerously-skip-permissions".into()
        }),
        (
            "outside the project and is not the folder your own settings name now",
            &|r| r["memory"]["dir"] = "/tmp/somewhere-else".into(),
        ),
        ("names no memory folder", &|r| {
            r["memory"] = serde_json::json!({})
        }),
    ] {
        let out = try_with(change);
        assert_eq!(out.code, 2, "{what}: {}", out.all());
        assert!(out.err.contains(what), "{what}: {}", out.all());
    }
    assert!(try_with(&|r| *r = serde_json::json!([1, 2]))
        .err
        .contains("it is not a JSON object"));
    assert_eq!(std::fs::read_dir(s.victim.join("run")).unwrap().count(), 1);
}

#[test]
fn looking_at_a_repository_runs_nothing_the_repository_names() {
    if !has_git() {
        return;
    }
    let s = Sandbox::new("git-config");
    s.write("app.py", "def main():\n    return 1\n");
    let hook = s
        .victim
        .join(if cfg!(windows) { "hook.cmd" } else { "hook.sh" });
    let flag = s.victim.join("PWNED");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", flag.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(&s.proj)
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q", "."]);
    git(&["config", "core.fsmonitor", &hook.display().to_string()]);
    for args in [
        &["map"][..],
        &["plan", "Pick the queue technology for the billing pipeline"],
        &["verify-locks", "--against", "HEAD"],
    ] {
        s.run(args);
    }
    assert!(!flag.exists(), "the program the repository named was run");
}

#[cfg(unix)]
#[test]
fn files_that_are_not_plain_files_are_not_read() {
    let s = Sandbox::new("odd-files");
    s.write("app.py", "def main():\n    return 1\n");
    std::fs::write(
        s.victim.join("secrets.py"),
        "def secret_function_name_xyz():\n    return 's3cr3t'\n",
    )
    .unwrap();
    link(s.victim.join("secrets.py"), s.proj.join("leak.py"));
    link("/dev/zero", s.proj.join("zero.py"));
    link(s.proj.clone(), s.proj.join("loop"));
    let made = std::process::Command::new("mkfifo")
        .arg(s.proj.join("pipe.py"))
        .status();
    let out = s.run(&["map"]);
    assert_eq!(out.code, 0, "{}", out.all());
    assert!(out.took < Duration::from_secs(10));
    let skeleton = std::fs::read_to_string(s.proj.join(".council/map/skeleton.md")).unwrap();
    assert!(skeleton.contains("app.py") && !skeleton.contains("secret_function_name_xyz"));
    assert!(!skeleton.contains("zero.py") && !skeleton.contains("leak.py"));
    // the same inside a git repository, where git lists the links as files
    if has_git() {
        std::process::Command::new("git")
            .current_dir(&s.proj)
            .args(["init", "-q", "."])
            .output()
            .unwrap();
        let out = s.run(&["map"]);
        assert_eq!(out.code, 0, "{}", out.all());
        let skeleton = std::fs::read_to_string(s.proj.join(".council/map/skeleton.md")).unwrap();
        assert!(!skeleton.contains("secret_function_name_xyz") && !skeleton.contains("zero.py"));
    }
    // settings that are a device or a pipe: an error at once, not a hang
    let _ = std::fs::remove_file(s.proj.join(".council").join("config.yaml"));
    link("/dev/zero", s.proj.join(".council").join("config.yaml"));
    let out = s.run(&["plan", "Pick the queue technology for the billing pipeline"]);
    assert_eq!(out.code, 0, "{}", out.all());
    assert!(
        out.out.contains(".council/config.yaml could not be read"),
        "{}",
        out.all()
    );
    let _ = made;
}

#[cfg(unix)]
#[test]
fn a_hostile_lock_manifest_is_checked_before_it_is_believed() {
    let s = Sandbox::new("locks");
    s.write("tests/test_a.py", "def test_a(): pass\n");
    link("/dev/zero", s.proj.join("tests").join("test_zero.py"));
    link(s.proj.join("tests"), s.proj.join("tests").join("again"));
    let out = s.run(&["lock-tests", "tests"]);
    assert_eq!(out.code, 0, "{}", out.all());
    assert!(out.out.contains("\"locked\": 1"), "{}", out.all());
    assert_eq!(s.run(&["lock-tests", "tests/test_zero.py"]).code, 2);
    std::fs::write(s.victim.join("secret.txt"), "topsecret\n").unwrap();
    s.write(".council/locks.json", r#"{"/dev/zero": "abc", "../victim/secret.txt": "x", "C:\\Windows\\win.ini": "y", "tests/test_a.py": "z"}"#);
    let out = s.run(&["verify-locks"]);
    assert_eq!(out.code, 1, "{}", out.all());
    assert!(out.took < Duration::from_secs(10));
    let found: serde_json::Value = serde_json::from_str(&out.out).unwrap();
    assert_eq!(
        found["refused"],
        serde_json::json!(["../victim/secret.txt", "/dev/zero", "C:\\Windows\\win.ini"])
    );
    assert_eq!(found["changed"], serde_json::json!(["tests/test_a.py"]));
    // the hook reads the same manifest: it answers at once and locks only what can be locked
    let event = format!(
        r#"{{"tool_name":"Edit","cwd":"{}","tool_input":{{"file_path":"/dev/zero"}}}}"#,
        s.proj.display()
    );
    let out = s.run_with(&["hook"], &[], Some(&event));
    assert_eq!((out.code, out.out.as_str()), (0, ""), "{}", out.all());
    s.write(".council/locks.json", "{not json");
    assert_eq!(s.run(&["verify-locks"]).code, 1);
    assert_eq!(s.run(&["lock-tests", "tests/test_a.py"]).code, 2);
}

#[test]
fn a_check_that_cannot_look_does_not_pass() {
    if !has_git() {
        return;
    }
    let s = Sandbox::new("against");
    s.write("tests/test_a.py", "def test_a(): pass\n");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(&s.proj)
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q", "."]);
    assert_eq!(s.run(&["lock-tests", "tests"]).code, 0);
    // a ref git does not know (a shallow clone in CI, a typo): the check fails, it does not pass
    let out = s.run(&["verify-locks", "--against", "origin/main"]);
    assert_eq!(out.code, 1, "{}", out.all());
    assert!(out.out.contains("cannot read origin/main"), "{}", out.all());
    // an option is not a ref
    let out = s.run(&["verify-locks", "--against=--output=/tmp/x"]);
    assert_eq!(out.code, 2, "{}", out.all());
    // a ref that exists and holds the manifest: the test edited together with its lock is caught
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.org",
        "commit",
        "-q",
        "-m",
        "locked",
    ]);
    assert_eq!(s.run(&["verify-locks", "--against", "HEAD"]).code, 0);
    s.write("tests/test_a.py", "def test_a(): assert True  # weakened\n");
    assert_eq!(s.run(&["lock-tests", "tests"]).code, 0);
    assert_eq!(s.run(&["verify-locks"]).code, 0); // the working copy agrees with its own, new, lock
    let out = s.run(&["verify-locks", "--against", "HEAD"]);
    assert_eq!(out.code, 1, "{}", out.all());
    assert!(out.out.contains("tests/test_a.py"));
}

#[test]
fn files_made_to_be_slow_or_endless_are_answered_at_once() {
    let s = Sandbox::new("slow");
    // nested aliases that would expand to a billion nodes
    let mut bomb =
        String::from("a: &a [\"x\",\"x\",\"x\",\"x\",\"x\",\"x\",\"x\",\"x\",\"x\",\"x\"]\n");
    let mut prev = 'a';
    for name in "bcdefghi".chars() {
        bomb.push_str(&format!(
            "{name}: &{name} [{}]\n",
            vec![format!("*{prev}"); 10].join(",")
        ));
        prev = name;
    }
    s.write(
        ".council/config.yaml",
        &format!("{bomb}members:\n  research:\n    seats: 2\n"),
    );
    let out = s.run(&["plan", "Pick the queue technology for the billing pipeline"]);
    assert_eq!(out.code, 0, "{}", out.all());
    assert!(out.took < Duration::from_secs(5));
    assert!(
        out.out.contains(".council/config.yaml could not be read"),
        "{}",
        out.all()
    );
    // patterns: one this tool will not run, and one that looks slow but is matched in linear time
    s.write(".council/config.yaml", "tripwires:\n  lookaround: \"(?<!x)billing\"\n  evil: \"(a+)+$\"\n  fine: \"\\\\bledger\\\\b\"\ntripwire_paths:\n  globby: [\"*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*b\"]\n");
    let ask = format!("Tidy the ledger helper {}!", "a".repeat(5000));
    let out = s.run(&[
        "plan",
        &ask,
        "--stakes",
        "low",
        "--paths",
        &"a".repeat(3000),
    ]);
    assert_eq!(out.code, 0, "{}", out.all());
    assert!(out.took < Duration::from_secs(5), "{:?}", out.took);
    assert!(
        out.out
            .contains("tripwire 'lookaround' in .council/config.yaml was ignored"),
        "{}",
        out.all()
    );
    assert!(
        out.out.contains("raised by tripwire(s): fine"),
        "{}",
        out.all()
    );
    // JSON nested a hundred thousand levels deep: an error, not a crash
    s.write(
        "deep.json",
        &format!("{}{}", "[".repeat(100_000), "]".repeat(100_000)),
    );
    let out = s.run(&["tally", "deep.json", "deep.json"]);
    assert_eq!(out.code, 1, "{}", out.all());
    assert!(
        out.err.contains("not valid JSON") && !out.err.contains("panicked"),
        "{}",
        out.all()
    );
    let event = format!(
        "{{\"tool_name\":\"Edit\",\"tool_input\":{}{}}}",
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    let out = s.run_with(&["hook"], &[], Some(&event));
    assert_eq!((out.code, out.out.as_str()), (0, ""));
    // source lines written to make a pattern reader backtrack
    let s = Sandbox::new("slow-code");
    s.write(
        "a.ts",
        &format!(
            "export function {}\nexport const {} = {}\n",
            "a".repeat(60_000),
            "x".repeat(30_000),
            "(".repeat(30_000)
        ),
    );
    s.write(
        "b.go",
        &format!(
            "func {}\ntype {} struct\n",
            "(".repeat(50_000),
            "T".repeat(50_000)
        ),
    );
    s.write(
        "c.rs",
        &format!(
            "pub fn {}{}\nimpl{}X\n",
            "f".repeat(50_000),
            "<".repeat(50_000),
            " ".repeat(80_000)
        ),
    );
    s.write(
        "d.sql",
        &format!(
            "CREATE TABLE {}IF NOT EXISTS{}t (\n",
            " ".repeat(90_000),
            " ".repeat(90_000)
        ),
    );
    s.write(
        "f.c",
        &format!(
            "int {} f{}\n#include {}\n{}",
            "*".repeat(60_000),
            "(".repeat(60_000),
            "<".repeat(90_000),
            "\n".repeat(200_000)
        ),
    );
    let out = s.run(&["map"]);
    assert_eq!(out.code, 0, "{}", out.all());
    assert!(out.took < Duration::from_secs(10), "{:?}", out.took);
}

#[cfg(unix)]
#[test]
fn text_that_drives_a_terminal_never_reaches_one() {
    let s = Sandbox::new("escapes");
    s.write("ok.py", "def other():\n    \"\"\"Doc with \x1b]0;title-hijack\x07 and \x1b[5mblink\x1b[0m.\"\"\"\n    return 2\n");
    s.write(
        "evil\x1b[2J\x1b[1;31mPWNED\x1b[0m.py",
        "def main():\n    return 1\n",
    );
    let out = s.run(&[
        "map",
        "--focus",
        "Why does main in evil return 1 and other return 2",
    ]);
    assert!(
        !out.all().contains('\x1b') && !out.all().contains('\x07'),
        "{:?}",
        out.all()
    );
    let skeleton =
        std::fs::read_to_string(s.proj.join(".council/map/skeleton.md")).unwrap_or_default();
    assert!(!skeleton.contains('\x1b'));
    // cards: what a judge reads and what is printed
    let cards = serde_json::json!([
        {"id": "c1", "title": "Nice \x1b[2J\x1b[31mtitle", "body": vec!["word"; 100].join(" "), "eli5": "simple \x07 bell", "author": "L1"},
        {"id": "c2", "title": "Other \u{202e}desrever", "body": vec!["word"; 100].join(" "), "eli5": "ok \u{e0041}", "author": "L2"}]);
    s.write("cards.json", &cards.to_string());
    let out = s.run(&["blind", "cards.json", "--seed", "1", "--out", "packet.md"]);
    assert_eq!(out.code, 0, "{}", out.all());
    let packet = std::fs::read_to_string(s.proj.join("packet.md")).unwrap();
    for odd in ['\x1b', '\x07', '\u{202e}', '\u{e0041}'] {
        assert!(!packet.contains(odd), "{odd:?} reached the judges' packet");
    }
    let out = s.run(&["blind", "cards.json"]);
    assert!(!out.out.contains('\x1b') && !out.out.contains('\u{202e}'));
}

/// A run that stands at the ideate step, with one seat's packet written.
#[cfg(unix)]
fn at_ideate(name: &str) -> (Sandbox, Stage) {
    let s = Sandbox::new(name);
    let stage = Stage::new(&s.proj, &s.base.join("own"), &common::council())
        .unwrap()
        .with_env("CI", None);
    let init = stage
        .council(&[
            "init",
            "Pick the queue technology for the billing pipeline",
            "--tier",
            "lite",
            "--mode",
            "fanout:claude",
            "--no-research",
            "--map",
            "never",
        ])
        .unwrap();
    assert_eq!(init.code, 0, "{}{}", init.out, init.err);
    std::fs::write(
        stage.run_dir().unwrap().join("brief.yaml"),
        script::brief(&stage.run().unwrap()),
    )
    .unwrap();
    let ingest = stage.council(&["ingest"]).unwrap();
    assert_eq!(ingest.code, 0, "{}{}", ingest.out, ingest.err);
    assert_eq!(stage.run().unwrap()["phase"], "ideate");
    (s, stage)
}

#[cfg(unix)]
#[test]
fn a_seat_output_is_a_plain_file_of_a_sane_size_or_it_is_not_read() {
    let (s, stage) = at_ideate("seat-output");
    let ideate = stage.run_dir().unwrap().join("loop-1").join("ideate");
    script::play_seats(&ideate).unwrap();
    // one seat "answers" with a link to a private file that happens to be valid JSON
    std::fs::write(
        s.victim.join("private.json"),
        r#"{"lens": "L1", "ideas": ["TOPSECRET-TOKEN-12345"], "cards": []}"#,
    )
    .unwrap();
    std::fs::remove_file(ideate.join("L1.out.json")).unwrap();
    link(s.victim.join("private.json"), ideate.join("L1.out.json"));
    // another with a file far too large
    std::fs::write(
        ideate.join("L2.out.json"),
        format!("{{\"ideas\": [\"{}\"]}}", "x".repeat(2_100_000)),
    )
    .unwrap();
    let out = stage.council(&["ingest"]).unwrap();
    let said = format!("{}{}", out.out, out.err);
    assert_ne!(out.code, 0, "{said}");
    assert!(
        said.contains("L1: REJECTED") && said.contains("the output must be a plain file"),
        "{said}"
    );
    assert!(said.contains("the limit is 2,000,000"), "{said}");
    assert!(
        !Sandbox::holds(&s.proj, "TOPSECRET-TOKEN-12345"),
        "the private file's content reached the project"
    );
    assert!(s.victim.join("private.json").exists());
}

#[cfg(unix)]
#[test]
fn who_wrote_which_card_is_for_the_owner_alone() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_s, stage) = at_ideate("private");
    let d = stage.run_dir().unwrap();
    script::play_seats(&d.join("loop-1").join("ideate")).unwrap();
    script::play_seats(&d.join("control")).unwrap();
    assert_eq!(stage.council(&["ingest"]).unwrap().code, 0);
    // the pool is approved; the cards are blinded for the red team: now there is something to hide
    script::approve_pool(&d.join("loop-1").join("pool.json")).unwrap();
    let blinded = stage.council(&["ingest"]).unwrap();
    assert_eq!(blinded.code, 0, "{}{}", blinded.out, blinded.err);
    let private = stage.own.join("private");
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&private), 0o700);
    let files: Vec<_> = std::fs::read_dir(&private)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(mode(&files[0]), 0o600);
}

#[test]
fn keys_are_never_written_into_a_log() {
    let s = Sandbox::new("secrets");
    let key = "sk-or-v1-0123456789abcdef0123456789abcdef";
    let log = s.base.join("council.log");
    let env = [
        ("OPENROUTER_API_KEY", key),
        ("COUNCIL_LOG", "trace"),
        ("COUNCIL_LOG_FILE", &*log.display().to_string()),
    ];
    for args in [
        &["doctor"][..],
        &["plan", "Pick the queue technology for the billing pipeline"],
        &[
            "ask",
            "--provider",
            "openrouter",
            "--model",
            "x/y",
            "--prompt-file",
            "nope.md",
            "--dry-run",
        ],
    ] {
        let out = s.run_with(args, &env, None);
        assert!(!out.all().contains(key), "{}", out.all());
    }
    let written = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(written.contains("[council info gate]"), "{written}");
    assert!(!written.contains(key));
}

#[test]
fn a_server_started_for_one_project_serves_no_other() {
    let s = Sandbox::new("mcp-fence");
    let other = Sandbox::new("mcp-fence-other");
    let call = |name: &str, args: serde_json::Value| {
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string()
    };
    let input = [
        call("council_status", serde_json::json!({"project_dir": other.proj.display().to_string()})),
        call("council_memory_add", serde_json::json!({"project_dir": other.proj.display().to_string(), "claim": "Queue A keeps order within one partition only", "source": "https://example.org/a"})),
        // places named in other arguments are fenced in too
        call("council_memory_import", serde_json::json!({"from": other.proj.display().to_string()})),
        call("council_map", serde_json::json!({"action": "install", "file": other.proj.join("map.md").display().to_string()})),
        call("council_memory_setup", serde_json::json!({"files_dir": other.victim.display().to_string(), "for_project": true})),
        // and your own settings are yours: a fenced server does not write them
        call("council_memory_setup", serde_json::json!({"files_dir": "notes"})),
        call("council_memory_status", serde_json::json!({})),
    ]
    .join("\n");
    std::fs::write(other.proj.join("map.md"), "# A map\n").unwrap();
    let out = s.run_with(
        &[
            "mcp",
            "--project",
            &s.proj.display().to_string(),
            "--toolsets",
            "all",
        ],
        &[],
        Some(&format!("{input}\n")),
    );
    let replies: Vec<serde_json::Value> = out
        .out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 7, "{}", out.all());
    for refused in &replies[..5] {
        assert_eq!(refused["result"]["isError"], true, "{refused}");
        assert!(
            refused["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("outside the folders this server works in"),
            "{refused}"
        );
    }
    assert_eq!(replies[5]["result"]["isError"], true, "{}", replies[5]);
    assert!(
        replies[5]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("run `council memory setup` yourself"),
        "{}",
        replies[5]
    );
    assert_eq!(
        replies[6]["result"]["structuredContent"]["ok"], true,
        "{}",
        replies[6]
    );
    assert!(!other.proj.join(".council").exists());
    assert!(
        !s.base
            .join("home")
            .join(".council")
            .join("config.yaml")
            .exists(),
        "the server wrote the person's own settings"
    );
    assert_eq!(std::fs::read_dir(&other.victim).unwrap().count(), 0);
}
