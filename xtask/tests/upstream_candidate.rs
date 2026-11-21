//! Real local Git/Cargo candidates must not replace the caller's compiled tooling.
#![cfg(unix)]

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("cadcraftgost-upstream-test-{}-{nonce}", std::process::id()));
        fs::create_dir_all(root.join(".cargo")).unwrap();
        fs::create_dir_all(root.join("xtask/src")).unwrap();
        let repo = Self(root);
        repo.git(&["init", "--initial-branch=main"]);
        repo.write("Cargo.toml", "[workspace]\nmembers = [\"xtask\"]\nresolver = \"3\"\n");
        repo.write(".cargo/config.toml", "[alias]\nxtask = \"run --quiet --offline --manifest-path xtask/Cargo.toml --\"\n");
        repo.write("xtask/Cargo.toml", "[package]\nname = \"xtask\"\nversion = \"0.0.0\"\nedition = \"2024\"\n");
        repo.write(
            "xtask/src/main.rs",
            r#"fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let value = std::fs::read_to_string(root.join("fixture.txt")).unwrap();
    if std::env::args().nth(1).as_deref() == Some("ci") {
        assert_eq!(value, "candidate", "CI must read the candidate's files, not its caller's");
    }
    println!("{value}");
}
"#,
        );
        repo.write("fixture.txt", "root");
        repo
    }

    fn write(&self, path: &str, content: &str) {
        fs::write(self.0.join(path), content).unwrap();
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(&self.0)
            .args(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        succeeded(&output);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn revision(&self, parent: Option<&str>, branch: &str) -> String {
        for path in ["Cargo.toml", ".cargo/config.toml", "xtask/Cargo.toml", "xtask/src/main.rs", "fixture.txt"] {
            let blob = self.git(&["hash-object", "-w", path]);
            self.git(&["update-index", "--add", "--cacheinfo", "100644", &blob, path]);
        }
        let tree = self.git(&["write-tree"]);
        let mut command = Command::new("git");
        command.current_dir(&self.0).args(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit-tree", &tree]);
        if let Some(parent) = parent {
            command.args(["-p", parent]);
        }
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(b"isolated fixture\n").unwrap();
        let output = child.wait_with_output().unwrap();
        succeeded(&output);
        let revision = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        self.git(&["update-ref", &format!("refs/heads/{branch}"), &revision]);
        revision
    }

    fn check(&self) -> Output {
        Command::new("bash")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("scripts/check-upstream.sh"))
            .arg(&self.0)
            .env("CADCRAFTGOST_UPSTREAM_URL", &self.0)
            .env("CADCRAFTGOST_UPSTREAM_BRANCH", "upstream")
            // Even an explicit caller target must not be inherited by the candidate.
            .env("CARGO_TARGET_DIR", self.0.join("caller-target"))
            .output()
            .unwrap()
    }

    fn probe(&self) -> String {
        let output = Command::new("cargo")
            .current_dir(&self.0)
            .args(["xtask", "probe"])
            .env("CARGO_TARGET_DIR", self.0.join("caller-target"))
            .output()
            .unwrap();
        succeeded(&output);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn succeeded(output: &Output) {
    assert!(output.status.success(), "stdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

#[test]
fn candidate_cache_is_private_and_survives_repeated_worktree_cleanup() {
    let repo = Repository::new();
    let base = repo.revision(None, "main");
    assert_eq!(repo.probe(), "root");
    repo.write("fixture.txt", "candidate");
    repo.revision(Some(&base), "upstream");
    repo.write("fixture.txt", "root");
    repo.git(&["read-tree", &base]);
    for _ in 0..2 {
        succeeded(&repo.check());
        assert_eq!(repo.probe(), "root", "the caller's compiled tool must survive candidate cleanup");
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), base, "a successful check must not adopt the candidate");
    }
}

#[test]
fn conflicting_or_unbuildable_candidates_never_change_the_caller() {
    for conflict in [false, true] {
        let repo = Repository::new();
        let base = repo.revision(None, "main");
        assert_eq!(repo.probe(), "root");
        let original = fs::read_to_string(repo.0.join("xtask/src/main.rs")).unwrap();
        repo.write("fixture.txt", "candidate");
        if !conflict {
            repo.write("xtask/src/main.rs", "compile_error!(\"deliberately unbuildable candidate\"); fn main() {}\n");
        }
        repo.revision(Some(&base), "upstream");
        repo.git(&["read-tree", &base]);
        // Restore the caller's source, not the uncommitted candidate fixture.
        repo.write("xtask/src/main.rs", &original);
        let expected = if conflict { "fork" } else { "root" };
        repo.write("fixture.txt", expected);
        let head = if conflict { repo.revision(Some(&base), "main") } else { base };
        let result = repo.check();
        assert!(!result.status.success(), "conflict or real Rust compilation failure must reject the candidate");
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(repo.probe(), expected);
    }
}
