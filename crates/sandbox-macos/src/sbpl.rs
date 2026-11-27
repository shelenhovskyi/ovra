//! A [`Plan`] as a Seatbelt profile (SBPL), the text `sandbox-exec -p`
//! takes.
//!
//! The profile denies everything, then allows: what every process needs
//! to start (the base rules), reading outside the hidden folders plus what
//! the plan gives back, and writing the plan's folders. What a rule must
//! not reach is a `require-not` condition inside it, so the result does not
//! depend on how Seatbelt orders rules. Folders a rule gives are matched as
//! written (`subpath`, `literal`); folders it keeps out are matched as
//! patterns without case ([`Pattern`]), so another spelling of a hidden or
//! protected path is kept out too, and another spelling of an allowed one
//! is merely refused.
//!
//! Paths are written into the text, so a path with a `"`, a `\` or a
//! control character is refused rather than escaped.

use std::fmt::Write as _;
use std::path::Path;

use ovra_core::services::Result;

use crate::invalid;
use crate::plan::{Net, Pattern, Plan};

/// Where the helper puts the bridge's port, once it has one.
pub(crate) const PORT_MARK: &str = "@OVRA_PROXY_PORT@";

/// The keychain's git credential helper, by name anywhere and in any
/// case: it may neither run nor be read, so it cannot be copied and run
/// from elsewhere (the keychain trusts its signature, not its path).
const KEYCHAIN_HELPER: &str = r#"(regex #"/[gG][iI][tT]-[cC][rR][eE][dD][eE][nN][tT][iI][aA][lL]-[oO](s|S|ſ)[xX](k|K|K)[eE][yY][cC][hH][aA][iI][nN]$")"#;

/// What every sandboxed process needs, whatever the session: to run
/// programs and fork, signal and inspect its own processes, read system
/// settings, use `/dev/null` and the ptys it opens itself (no `/dev/tty`:
/// the helper leaves the person's terminal with `setsid`), and look up the
/// few system services that resolve users, read preferences, log, and
/// check certificates. Not the keychain (`com.apple.SecurityServer`): it
/// would answer for any item `/usr/bin/security` may read, the person's
/// `gh` login among them, so `claude` in the sandbox keeps its login in
/// its file instead. No service that starts programs outside the sandbox
/// (Launch Services, `launchd` jobs) and no DNS: names are resolved by the
/// netproxy.
const BASE: &str = r#"(version 1)
(deny default)
(allow process-exec
  (require-not (regex #"/[gG][iI][tT]-[cC][rR][eE][dD][eE][nN][tT][iI][aA][lL]-[oO](s|S|ſ)[xX](k|K|K)[eE][yY][cC][hH][aA][iI][nN]$")))
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))
(allow sysctl-read)
(allow user-preference-read)
(allow ipc-posix-sem)
(allow ipc-posix-shm-read* (ipc-posix-name-prefix "apple.cfprefs."))
(allow pseudo-tty)
(allow file-read-metadata)
(allow file-map-executable)
(allow file-read* file-write*
  (literal "/dev/null")
  (literal "/dev/zero")
  (regex #"^/dev/fd/[0-9]+$"))
(allow file-read* file-write* file-ioctl
  (literal "/dev/ptmx")
  (literal "/dev/dtracehelper"))
(allow file-read* file-write* file-ioctl
  (require-all
    (regex #"^/dev/ttys[0-9]+$")
    (extension "com.apple.sandbox.pty")))
(allow mach-lookup
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.opendirectoryd.membership")
  (global-name "com.apple.system.notification_center")
  (global-name "com.apple.system.logger")
  (global-name "com.apple.logd")
  (global-name "com.apple.cfprefsd.daemon")
  (global-name "com.apple.cfprefsd.agent")
  (global-name "com.apple.bsd.dirhelper")
  (global-name "com.apple.trustd")
  (global-name "com.apple.trustd.agent")
  (global-name "com.apple.ocspd"))
"#;

/// The base rules with everything readable and nothing writable: what
/// [`crate::Seatbelt::detect`] tries once.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn probe() -> String {
    format!("{BASE}(allow file-read*)\n")
}

/// The profile of `plan`.
pub(crate) fn profile(plan: &Plan) -> Result<String> {
    let mut text = String::from(BASE);
    // Reading: anywhere outside the hidden folders, and the folders and
    // files the plan gives back; never what it keeps out.
    let mut shown = Vec::new();
    for path in plan.read.iter().chain(&plan.write) {
        shown.push(format!("(subpath {})", string(path)?));
    }
    for path in plan.read_files.iter().chain(&plan.write_files) {
        shown.push(format!("(literal {})", string(path)?));
    }
    let hidden = plan
        .hidden
        .iter()
        .map(|path| regex(&Pattern::Under(path.clone())))
        .collect::<Result<Vec<_>>>()?;
    let never = plan.never.iter().map(regex).collect::<Result<Vec<_>>>()?;
    let mut readable = vec![format!("(require-not {})", any(&hidden))];
    readable.extend(shown);
    let mut unreadable = never.clone();
    unreadable.push(KEYCHAIN_HELPER.to_owned());
    text.push_str("(allow file-read*\n  (require-all\n    ");
    text.push_str(&any(&readable));
    let _ = write!(text, "\n    (require-not {})", any(&unreadable));
    text.push_str("))\n");
    // Writing: only the plan's folders and files, never a protected path
    // or one kept out.
    let mut writable = Vec::new();
    for path in &plan.write {
        writable.push(format!("(subpath {})", string(path)?));
    }
    for path in &plan.write_files {
        writable.push(format!("(literal {})", string(path)?));
    }
    if !writable.is_empty() {
        let mut kept_out = plan
            .protected
            .iter()
            .map(regex)
            .collect::<Result<Vec<_>>>()?;
        kept_out.extend(never);
        text.push_str("(allow file-write*\n  (require-all\n    ");
        text.push_str(&any(&writable));
        if !kept_out.is_empty() {
            let _ = write!(text, "\n    (require-not {})", any(&kept_out));
        }
        text.push_str("))\n");
    }
    if let Some(sockets) = &plan.sockets {
        let at = string(sockets)?;
        let _ = writeln!(
            text,
            "(allow network-bind network-inbound (local unix-socket (subpath {at})))"
        );
        let _ = writeln!(
            text,
            "(allow network-outbound (remote unix-socket (subpath {at})))"
        );
    }
    match &plan.net {
        Net::Blocked => {}
        Net::Bridge(_) => {
            let _ = writeln!(
                text,
                "(allow network-outbound (remote ip \"localhost:{PORT_MARK}\"))"
            );
        }
        Net::Open => {
            // Any address, but Unix sockets only in the temporary folder
            // (above): not the Docker socket or another app's.
            text.push_str("(allow network-outbound (remote ip \"*:*\"))\n(allow system-socket)\n");
            text.push_str("(allow mach-lookup (global-name \"com.apple.dnssd.service\"))\n");
        }
    }
    Ok(text)
}

/// `(require-any ...)` of `items`, or the one item alone.
fn any(items: &[String]) -> String {
    match items {
        [one] => one.clone(),
        _ => format!("(require-any\n      {})", items.join("\n      ")),
    }
}

/// `path` as an SBPL string.
fn string(path: &Path) -> Result<String> {
    Ok(format!("\"{}\"", text(path)?))
}

/// `path` as text that can go between quotes in the profile.
fn text(path: &Path) -> Result<&str> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid(format!("{} is not valid UTF-8", path.display())))?;
    // The helper puts the bridge's port where the mark is.
    if text
        .chars()
        .any(|c| c == '"' || c == '\\' || c.is_control())
        || text.contains(PORT_MARK)
    {
        return Err(invalid(format!(
            "the macOS sandbox cannot use {}: its name holds a quote, a backslash or a control character",
            path.display()
        )));
    }
    Ok(text)
}

/// `pattern` as an SBPL regex that ignores case.
fn regex(pattern: &Pattern) -> Result<String> {
    let body = match pattern {
        Pattern::Under(path) => format!("^{}(/.*)?$", fold(text(path)?)),
        Pattern::Named { root, name } => {
            format!("^{}/(.*/)?{}(/.*)?$", fold(text(root)?), fold(name))
        }
    };
    Ok(format!("(regex #\"{body}\")"))
}

/// `text` as a regex that matches it literally, each ASCII letter in
/// either case, and `s` and `k` also as the letters APFS folds to them
/// (U+017F long s, U+212A Kelvin sign), written as alternatives because
/// the regex works on bytes.
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        match c {
            's' | 'S' => out.push_str("(s|S|\u{17f})"),
            'k' | 'K' => out.push_str("(k|K|\u{212a})"),
            'a'..='z' | 'A'..='Z' => {
                let _ = write!(
                    out,
                    "[{}{}]",
                    c.to_ascii_lowercase(),
                    c.to_ascii_uppercase()
                );
            }
            '.' | '*' | '+' | '?' | '(' | ')' | '|' | '{' | '}' | '$' | '[' | ']' => {
                let _ = write!(out, "[{c}]");
            }
            // In brackets it would negate them.
            '^' => out.push_str("\\^"),
            c => out.push(c),
        }
    }
    out
}

/// [`fold`], for the plan's tests to read profiles back.
#[cfg(test)]
pub(crate) fn fold_for_tests(text: &str) -> String {
    fold(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_matches_its_path_in_any_case_and_nothing_else_by_accident() {
        assert_eq!(fold("a.B"), "[aA][.][bB]");
        assert_eq!(fold("sk"), "(s|S|\u{17f})(k|K|\u{212a})");
        assert_eq!(fold("x^y]"), r"[xX]\^[yY][]]");
        let named = Pattern::Named {
            root: "/w".into(),
            name: ".git".into(),
        };
        assert_eq!(
            regex(&named).ok().as_deref(),
            Some(r#"(regex #"^/[wW]/(.*/)?[.][gG][iI][tT](/.*)?$")"#)
        );
    }

    #[test]
    fn no_profile_reaches_the_keychain() {
        // The keychain would answer for every item `security` may read (the
        // gh login): every system service any profile names is one of these.
        let allowed = [
            "com.apple.system.opendirectoryd.libinfo",
            "com.apple.system.opendirectoryd.membership",
            "com.apple.system.notification_center",
            "com.apple.system.logger",
            "com.apple.logd",
            "com.apple.cfprefsd.daemon",
            "com.apple.cfprefsd.agent",
            "com.apple.bsd.dirhelper",
            "com.apple.trustd",
            "com.apple.trustd.agent",
            "com.apple.ocspd",
            "com.apple.dnssd.service",
        ];
        for net in [Net::Blocked, Net::Bridge("/s".into()), Net::Open] {
            let plan = Plan::for_tests(net);
            let text = profile(&plan).ok().unwrap_or_default();
            assert!(text.contains("(deny default)"));
            for part in text.split("(global-name \"").skip(1) {
                let name = part.split('"').next().unwrap_or_default();
                assert!(allowed.contains(&name), "{name}");
            }
            assert!(!text.contains("SecurityServer") && !text.contains("securityd"));
        }
    }

    #[test]
    fn the_keychain_helper_is_matched_in_any_case_by_exec_and_read() {
        let pattern = format!("(regex #\"/{}$\")", fold("git-credential-osxkeychain"));
        assert_eq!(KEYCHAIN_HELPER, pattern);
        assert!(BASE.contains(&format!("(allow process-exec\n  (require-not {pattern}))")));
    }

    #[test]
    fn a_path_that_would_need_escaping_is_refused() {
        for bad in ["/w/a\"b", "/w/a\\b", "/w/a\nb"] {
            assert!(string(Path::new(bad)).is_err(), "{bad:?}");
            assert!(regex(&Pattern::Under(bad.into())).is_err(), "{bad:?}");
        }
        assert_eq!(
            string(Path::new("/w/a b")).ok().as_deref(),
            Some("\"/w/a b\"")
        );
    }
}
