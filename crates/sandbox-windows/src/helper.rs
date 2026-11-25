//! The helper: Ovra's own executable started with [`HELPER_FLAG`].
//!
//! 1. As the person, it applies the plan's ACEs and starts itself as the
//!    sandbox user with [`MIDDLE_FLAG`].
//! 2. As the sandbox user, it checks that the firewall rule holds, makes the
//!    session's write-restricted token and starts itself under it with
//!    [`INNER_FLAG`].
//! 3. Under the token, it starts `claude`.
//!
//! Without `--data` (tests) step 1 starts step 2 as the person. Every
//! process of the session is in the helper's job: when the helper ends,
//! they end. The setup flags run [`setup`](crate::win::setup).

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use crate::HELPER_FLAG;
use crate::plan::Right;
use crate::win::{Sid, acl, logon, setup, token};

/// The first argument of the helper's middle stage, as the sandbox user.
pub(crate) const MIDDLE_FLAG: &str = "--ovra-sandbox-middle";
/// The first argument of the helper's last stage, under the token.
pub(crate) const INNER_FLAG: &str = "--ovra-sandbox-inner";
/// Sets the sandbox up for a data folder (one UAC prompt).
pub(crate) const SETUP_FLAG: &str = "--ovra-sandbox-setup";
/// Removes the sandbox user and its firewall rule (elevated).
pub(crate) const REMOVE_FLAG: &str = "--ovra-sandbox-remove";

/// Public addresses, IPv4 and IPv6, the sandbox user must not reach: if
/// it can, the firewall rule does not hold and the session must not start.
const OUTSIDE: [&str; 2] = ["1.1.1.1:443", "[2606:4700:4700::1111]:443"];

/// The helper's arguments, as `Plan::line` writes them.
#[derive(Debug, Default)]
struct Args {
    sid: String,
    data: Option<PathBuf>,
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
            "--data" => args.data = Some(value.into()),
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
    let report = |result: Result<u32, String>| match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ovra sandbox: {error}");
            126
        }
    };
    let code = if first == HELPER_FLAG {
        report(parse(raw).and_then(|args| outer(&args)))
    } else if first == MIDDLE_FLAG {
        report(parse(raw).and_then(|args| middle(&args)))
    } else if first == INNER_FLAG {
        inner(raw)
    } else if first == SETUP_FLAG {
        let data = raw.next().map(PathBuf::from).unwrap_or_default();
        report(setup::setup(&data).map(|()| 0).map_err(|e| e.to_string()))
    } else if first == setup::ADMIN_FLAG {
        let _data = raw.next();
        let handoff = raw.next().map(PathBuf::from).unwrap_or_default();
        report(
            setup::admin(&handoff)
                .map(|()| 0)
                .map_err(|e| e.to_string()),
        )
    } else if first == REMOVE_FLAG {
        report(setup::remove().map(|()| 0).map_err(|e| e.to_string()))
    } else {
        return;
    };
    std::process::exit(code as i32);
}

fn io(error: std::io::Error) -> String {
    error.to_string()
}

/// As the person: applies the plan and starts the middle stage.
fn outer(args: &Args) -> Result<u32, String> {
    let project = Sid::parse(&args.sid).map_err(io)?;
    let user = match &args.data {
        Some(_) => Some(logon::account_sid(setup::USER).map_err(io)?),
        None => None,
    };
    for (path, right) in &args.grants {
        if let Some(user) = &user {
            acl::grant(path, user, *right).map_err(io)?;
        }
        if *right == Right::Modify {
            acl::grant(path, &project, *right).map_err(io)?;
        }
    }
    for path in &args.denied {
        acl::deny(path, &project).map_err(io)?;
        if let Some(user) = &user {
            acl::deny(path, user).map_err(io)?;
        }
    }
    let env: Vec<(OsString, OsString)> = args
        .kept
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (name.clone(), value)))
        .collect();
    let me = std::env::current_exe().map_err(io)?;
    let mut next = vec![
        OsString::from(MIDDLE_FLAG),
        "--sid".into(),
        (&args.sid).into(),
        "--".into(),
        args.program.clone().into(),
    ];
    next.extend(args.args.iter().cloned());
    let cwd = std::env::current_dir().map_err(io)?;
    match &args.data {
        Some(data) => {
            let password = setup::password(data)
                .map_err(|e| format!("the Windows sandbox is not set up: {e}"))?;
            setup::firewall_holds().map_err(io)?;
            logon::run_as(setup::USER, &password, &me, &next, &env, &cwd).map_err(io)
        }
        None => {
            let token = token::restricted(&args.sid).map_err(io)?;
            let mut inner = vec![OsString::from(INNER_FLAG), args.program.clone().into()];
            inner.extend(args.args.iter().cloned());
            token::run(&token, &me, &inner, &env, &cwd).map_err(io)
        }
    }
}

/// As the sandbox user: checks the firewall rule, then starts the last
/// stage under the session's token.
fn middle(args: &Args) -> Result<u32, String> {
    let reached = OUTSIDE
        .iter()
        .filter_map(|a| a.parse().ok())
        .any(|address| {
            std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(1500))
                .is_ok()
        });
    if reached {
        return Err("the sandbox user reaches the internet: its firewall rule is missing. Set the Windows sandbox up again".into());
    }
    let token = token::restricted(&args.sid).map_err(io)?;
    token::close_to_account().map_err(io)?;
    let me = std::env::current_exe().map_err(io)?;
    let mut inner = vec![OsString::from(INNER_FLAG), args.program.clone().into()];
    inner.extend(args.args.iter().cloned());
    let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let cwd = std::env::current_dir().map_err(io)?;
    token::run(&token, &me, &inner, &env, &cwd).map_err(io)
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
