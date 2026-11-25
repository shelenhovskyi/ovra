//! The helper: Ovra's own executable started with [`HELPER_FLAG`]. It
//! applies the plan's ACEs, makes the session's write-restricted token and
//! runs itself again under it with [`INNER_FLAG`], which starts `claude`.
//! Every process of the session is in the helper's job: when the helper
//! ends, they end.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use crate::HELPER_FLAG;
use crate::plan::Right;
use crate::win::{Sid, acl, token};

/// The first argument of the helper's second stage, under the token.
pub(crate) const INNER_FLAG: &str = "--ovra-sandbox-inner";

/// The helper's arguments, as `Plan::line` writes them.
#[derive(Debug, Default)]
struct Args {
    sid: String,
    grants: Vec<(PathBuf, Right)>,
    denied: Vec<PathBuf>,
    kept: Vec<OsString>,
    program: PathBuf,
    args: Vec<OsString>,
}

fn parse(mut raw: impl Iterator<Item = OsString>) -> Result<Args, String> {
    let mut args = Args::default();
    while let Some(flag) = raw.next() {
        let flag = flag.to_string_lossy().into_owned();
        if flag == "--" {
            args.program = raw.next().ok_or("no program after --")?.into();
            args.args = raw.collect();
            return Ok(args);
        }
        let value = raw.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--sid" => args.sid = value.to_string_lossy().into_owned(),
            "--m" => args.grants.push((value.into(), Right::Modify)),
            "--r" => args.grants.push((value.into(), Right::Read)),
            "--rx" => args.grants.push((value.into(), Right::ReadExecute)),
            "--deny" => args.denied.push(value.into()),
            "--env" => args.kept.push(value),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Err("no -- before the program".into())
}

/// Call first in `main`: if this process was started as the sandbox's
/// helper, does that job and never returns.
pub fn helper_if_asked() {
    let mut raw = std::env::args_os().skip(1);
    let Some(first) = raw.next() else { return };
    let code = if first == HELPER_FLAG {
        match parse(raw).and_then(|args| outer(&args)) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("ovra sandbox: {error}");
                126
            }
        }
    } else if first == INNER_FLAG {
        inner(raw)
    } else {
        return;
    };
    std::process::exit(code as i32);
}

/// Applies the plan and runs the second stage under the session's token.
fn outer(args: &Args) -> Result<u32, String> {
    let sid = Sid::parse(&args.sid).map_err(|e| e.to_string())?;
    for (path, right) in &args.grants {
        if *right == Right::Modify {
            acl::grant(path, &sid, *right).map_err(|e| e.to_string())?;
        }
    }
    for path in &args.denied {
        acl::deny(path, &sid).map_err(|e| e.to_string())?;
    }
    let env: Vec<(OsString, OsString)> = args
        .kept
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (name.clone(), value)))
        .collect();
    let token = token::restricted(&args.sid).map_err(|e| e.to_string())?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut inner = vec![OsString::from(INNER_FLAG), args.program.clone().into()];
    inner.extend(args.args.iter().cloned());
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    token::run(&token, &me, &inner, &env, &cwd).map_err(|e| e.to_string())
}

/// Under the token: starts the program with the environment it was given.
fn inner(mut raw: impl Iterator<Item = OsString>) -> u32 {
    let Some(program) = raw.next() else {
        eprintln!("ovra sandbox: no program");
        return 126;
    };
    match Command::new(&program).args(raw).status() {
        Ok(status) => status.code().map_or(1, |c| c as u32),
        Err(error) => {
            eprintln!(
                "ovra sandbox: cannot start {}: {error}",
                program.to_string_lossy()
            );
            127
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_plan_arguments_read_back() {
        let raw = [
            "--sid",
            "S-1-5-21-1-2-3-4",
            "--m",
            "w",
            "--deny",
            "w/.git",
            "--env",
            "PATH",
            "--",
            "claude",
            "-p",
        ];
        let args = parse(raw.iter().map(OsString::from)).unwrap();
        assert_eq!(args.sid, "S-1-5-21-1-2-3-4");
        assert_eq!(args.grants, vec![(PathBuf::from("w"), Right::Modify)]);
        assert_eq!(args.denied, vec![PathBuf::from("w/.git")]);
        assert_eq!(args.program, PathBuf::from("claude"));
        assert_eq!(args.args, vec![OsString::from("-p")]);
        assert!(parse(["--m"].iter().map(OsString::from)).is_err());
        assert!(parse(["--what", "x", "--", "c"].iter().map(OsString::from)).is_err());
    }
}
