//! Speed of `GitWorktrees` on a large working tree, for the speed gate
//! (`tests/perf/specs/worktree.spec.ts`, `just perf`). Builds a repository
//! with 2,000 tracked and 10,000 untracked files in a temporary folder, has
//! an "agent" change 600 tracked files, and prints one JSON line per metric:
//! `{"name": "worktree.<metric>", "value": <ms>}`.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use ovra_core::path::RelPath;
use ovra_core::services::{Result, Worktrees};
use ovra_worktree::GitWorktrees;

const TRACKED: usize = 2_000;
const UNTRACKED: usize = 10_000;
const CHANGED: usize = 600;

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|e| fail(&e))?;
    runtime.block_on(run())
}

fn fail(e: &dyn std::fmt::Display) -> ovra_core::protocol::ProtocolError {
    ovra_core::protocol::ProtocolError::new(ovra_core::protocol::ErrorCode::Internal, e.to_string())
}

fn write(root: &Path, path: &str, text: &str) -> Result<()> {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| fail(&e))?;
    }
    fs::write(path, text).map_err(|e| fail(&e))
}

fn git(root: &Path, args: &[&str]) -> Result<()> {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=Bench",
            "-c",
            "user.email=bench@example.com",
        ])
        .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| fail(&e))?
        .status
        .success();
    if ok {
        Ok(())
    } else {
        Err(fail(&format!("git {args:?} failed")))
    }
}

fn tracked(n: usize) -> String {
    format!("packages/pkg-{:03}/src/module-{}.ts", n / 10, n % 10)
}

fn report(name: &str, started: Instant) {
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    println!("{{\"name\":\"worktree.{name}\",\"value\":{ms:.1}}}");
}

async fn run() -> Result<()> {
    let tmp = tempfile::tempdir().map_err(|e| fail(&e))?;
    let root = tmp.path().join("repo");
    fs::create_dir_all(&root).map_err(|e| fail(&e))?;
    git(&root, &["init", "--quiet"])?;
    for n in 0..TRACKED {
        write(&root, &tracked(n), &format!("export const v{n} = {n};\n"))?;
    }
    git(&root, &["add", "--all"])?;
    git(&root, &["commit", "--quiet", "-m", "start"])?;
    for n in 0..UNTRACKED {
        let path = format!("huge/dir-{:02}/file-{n}.txt", n / 1_000);
        write(&root, &path, &format!("generated {n}\n"))?;
    }

    let w = GitWorktrees::new();
    let started = Instant::now();
    let base = w.snapshot(root.clone()).await?;
    report("snapshot_first_ms", started);

    let paths: Vec<RelPath> = (0..CHANGED)
        .map(|n| RelPath::parse(&tracked(n)))
        .collect::<Result<_>>()?;
    for n in 0..CHANGED {
        write(
            &root,
            &tracked(n),
            &format!("export const v{n} = {};\n", n + 1),
        )?;
    }
    let started = Instant::now();
    w.snapshot(root.clone()).await?;
    report("snapshot_ms", started);

    let started = Instant::now();
    let changes = w.changes(root.clone(), base.clone()).await?;
    report("changes_ms", started);
    if changes.len() != CHANGED {
        return Err(fail(&format!(
            "{} changes, expected {CHANGED}",
            changes.len()
        )));
    }

    let started = Instant::now();
    w.patch(root.clone(), base.clone(), paths.clone()).await?;
    report("patch_600_ms", started);

    let started = Instant::now();
    w.restore(root.clone(), base.clone(), paths).await?;
    report("restore_600_ms", started);
    let left = w.changes(root.clone(), base).await?;
    if !left.is_empty() {
        return Err(fail(&format!("{} files not restored", left.len())));
    }
    Ok(())
}
