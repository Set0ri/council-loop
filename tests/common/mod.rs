//! What the integration tests share: a sandbox (a project, a home and a "victim" folder that
//! nothing may touch), and a way to run the binary in it with a time limit.
#![allow(dead_code)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn council() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_council"))
}

/// What one command did.
pub struct Out {
    pub code: i32,
    pub out: String,
    pub err: String,
    pub took: Duration,
}

impl Out {
    /// Standard output and standard error together.
    pub fn all(&self) -> String {
        format!("{}{}", self.out, self.err)
    }
}

/// A project folder, a home folder and a folder that stands for the rest of the machine.
pub struct Sandbox {
    pub base: PathBuf,
    pub proj: PathBuf,
    pub victim: PathBuf,
}

/// The folders go when the test is over. Keep them to look at with `COUNCIL_TEST_KEEP=1`.
impl Drop for Sandbox {
    fn drop(&mut self) {
        if std::env::var_os("COUNCIL_TEST_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }
}

impl Sandbox {
    pub fn new(name: &str) -> Sandbox {
        let dir = std::env::temp_dir().join(format!("council-it-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["proj", "victim", "home"] {
            std::fs::create_dir_all(dir.join(sub)).expect("the temporary folder is writable");
        }
        let base = dunce::canonicalize(&dir).expect("the folder was just made");
        Sandbox {
            proj: base.join("proj"),
            victim: base.join("victim"),
            base,
        }
    }

    /// Write a file in the project (folders are made as needed).
    pub fn write(&self, rel: &str, text: &str) -> PathBuf {
        let file = self.proj.join(rel);
        std::fs::create_dir_all(file.parent().expect("a file has a folder")).expect("folder");
        std::fs::write(&file, text).expect("write");
        file
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(council());
        cmd.args(args).current_dir(&self.proj);
        for (name, _) in std::env::vars_os() {
            let name = name.to_string_lossy().into_owned();
            if (name.starts_with("COUNCIL_") && name != "COUNCIL_TEST_KEEP")
                || matches!(
                    name.as_str(),
                    "CLAUDECODE"
                        | "OPENROUTER_API_KEY"
                        | "AA_API_KEY"
                        | "CI"
                        | "CLAUDE_PROJECT_DIR"
                )
            {
                cmd.env_remove(name);
            }
        }
        cmd.env("COUNCIL_HOME", self.base.join("home").join(".council"))
            .env("COUNCIL_PRIVATE_DIR", self.base.join("private"))
            .env("COUNCIL_NO_WELCOME", "1")
            .env("COUNCIL_NOW", "2026-11-13T12:00:00")
            .env("CLAUDECODE", "1");
        cmd
    }

    /// Run `council <args>` in the project. A command that is still running after 30 seconds is
    /// stopped and reported with exit code -9: nothing here may hang.
    pub fn run(&self, args: &[&str]) -> Out {
        self.run_with(args, &[], None)
    }

    pub fn run_with(&self, args: &[&str], env: &[(&str, &str)], stdin: Option<&str>) -> Out {
        let mut cmd = self.command(args);
        for (name, value) in env {
            cmd.env(name, value);
        }
        finish(cmd, stdin)
    }

    /// Does any file under `dir` hold `needle`?
    pub fn holds(dir: &Path, needle: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let path = entry.path();
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => Sandbox::holds(&path, needle),
                Ok(meta) if meta.is_file() => std::fs::read(&path)
                    .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(needle)),
                _ => false,
            }
        })
    }
}

/// Run a prepared command with a time limit.
pub fn finish(mut cmd: Command, stdin: Option<&str>) -> Out {
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = cmd.spawn().expect("the binary runs");
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = pipe.write_all(text.as_bytes());
        });
    }
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buffer);
            }
            String::from_utf8_lossy(&buffer).into_owned()
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    // look often at first (most commands are done within a few milliseconds, and some tests time
    // them), then less often
    let mut nap = Duration::from_micros(50);
    let code = loop {
        match child.try_wait().expect("wait") {
            Some(status) => break status.code().unwrap_or(-1),
            None if started.elapsed() > Duration::from_secs(30) => {
                let _ = child.kill();
                let _ = child.wait();
                break -9;
            }
            None => {
                std::thread::sleep(nap);
                nap = (nap * 2).min(Duration::from_millis(5));
            }
        }
    };
    Out {
        code,
        out: out.join().unwrap_or_default(),
        err: err.join().unwrap_or_default(),
        took: started.elapsed(),
    }
}

pub fn has_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}
