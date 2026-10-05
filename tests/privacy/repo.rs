//! Banco, chaves e arquivos de evidência ficam fora do Git.
//! Usa o próprio `git` do repositório; sem git ou fora de um repositório, o teste é pulado.

use std::path::Path;
use std::process::Command;

fn repo_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn git(args: &[&str]) -> Option<std::process::Output> {
    Command::new("git").args(args).current_dir(repo_root()).output().ok()
}

fn in_a_git_repo() -> bool {
    git(&["rev-parse", "--is-inside-work-tree"]).is_some_and(|o| o.status.success())
}

fn is_ignored(path: &str) -> bool {
    git(&["check-ignore", "-q", "--no-index", path]).is_some_and(|o| o.status.success())
}

#[test]
fn everything_the_app_writes_to_disk_is_ignored_by_git() {
    if !in_a_git_repo() {
        eprintln!("skipped: not a git repository");
        return;
    }
    let must_be_ignored = [
        "meta.db",
        "meta.db-wal",
        "key.bin",
        "recorder/seg-0000000001.bbseg",
        "recorder/seg-0000000001.keep",
        "recorder/journal.bbwal",
        "recorder/journal.bbwal.corrupt",
        "recorder/pruned.log",
        "exports/incident-1-1.json",
        "evidence/a.bin",
        "recordings/a.bin",
        ".env",
        ".env.local",
        "server.pem",
        "private.key",
        "app.log",
        "crash.dmp",
        "screenshots/a.png",
        "config.local.json",
        "backups/a.zip",
        "target/debug/x",
        "node_modules/x/index.js",
    ];
    for p in must_be_ignored {
        assert!(is_ignored(p), "{p} is not ignored by .gitignore");
    }
    for p in [".env.example", "README.md", "tests/privacy/guard.rs", "crates/bb-core/src/lib.rs", "scripts/check-tracked.mjs"] {
        assert!(!is_ignored(p), "{p} must NOT be ignored");
    }
}

#[test]
fn no_sensitive_file_is_tracked_by_git() {
    if !in_a_git_repo() {
        eprintln!("skipped: not a git repository");
        return;
    }
    let out = git(&["ls-files"]).expect("git ls-files");
    let files = String::from_utf8_lossy(&out.stdout);
    let banned_suffixes = [
        ".db", ".db-wal", ".db-shm", ".sqlite", ".sqlite3", ".bbseg", ".bbwal", ".corrupt", ".keep", ".log", ".dmp", ".pem", ".key",
        ".pfx", ".p12", ".bak", ".pcap", ".etl",
    ];
    for f in files.lines() {
        let lower = f.to_lowercase();
        assert!(!banned_suffixes.iter().any(|s| lower.ends_with(s)), "sensitive file is tracked: {f}");
        let name = lower.rsplit('/').next().unwrap_or("");
        assert!(name != "key.bin" && name != "pruned.log", "sensitive file is tracked: {f}");
        assert!(!(name == ".env" || (name.starts_with(".env.") && name != ".env.example")), "env file tracked: {f}");
        assert!(
            !["segments/", "evidence/", "exports/", "recordings/", "screenshots/"]
                .iter()
                .any(|d| lower.starts_with(d) || lower.contains(&format!("/{d}"))),
            "data folder content is tracked: {f}"
        );
    }
}
