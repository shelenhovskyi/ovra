//! What `ssh -G` says the person's config resolves to, and the check that
//! the hardened command line won over it.

use std::collections::BTreeMap;

use ovra_core::protocol::{ErrorCode, ProtocolError};

/// `ssh -G` output: lowercase keys, each with every value it printed.
#[derive(Debug, Default)]
pub(crate) struct Effective(BTreeMap<String, Vec<String>>);

impl Effective {
    pub(crate) fn parse(text: &str) -> Self {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for line in text.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            if !key.is_empty() {
                map.entry(key.to_ascii_lowercase())
                    .or_default()
                    .push(value.trim().to_owned());
            }
        }
        Self(map)
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.first()).map(String::as_str)
    }

    /// The name `ssh` checks host keys under: the alias, else the host
    /// name, as `[host]:port` when the port is not 22.
    pub(crate) fn key_name(&self) -> Option<String> {
        if let Some(alias) = self.get("hostkeyalias") {
            return Some(alias.to_owned());
        }
        let host = self.get("hostname")?;
        Some(match self.get("port") {
            Some("22") | None => host.to_owned(),
            Some(port) => format!("[{host}]:{port}"),
        })
    }

    /// The files `ssh` reads keys from: the person's and the system's.
    /// `ssh -G` joins them with spaces, so a path with a space comes back
    /// in pieces; only absolute pieces are kept, never one read relative
    /// to Ovra's own folder.
    pub(crate) fn known_files(&self) -> Vec<String> {
        ["userknownhostsfile", "globalknownhostsfile"]
            .iter()
            .filter_map(|key| self.get(key))
            .flat_map(|list| list.split(' ').map(str::to_owned))
            .filter(|file| std::path::Path::new(file).is_absolute())
            .collect()
    }

    /// Refuses unless every value that keeps the connection to its pins
    /// and keeps the laptop's credentials and ports to itself is as the
    /// command line set it.
    pub(crate) fn check(&self, pins: &str, alias: &str) -> Result<(), ProtocolError> {
        let wanted = [
            ("stricthostkeychecking", "true"),
            // The one way a server could write keys into the pin file.
            ("updatehostkeys", "false"),
            ("batchmode", "yes"),
            ("checkhostip", "no"),
            ("requesttty", "false"),
            ("globalknownhostsfile", "none"),
            ("verifyhostkeydns", "false"),
            ("gssapidelegatecredentials", "no"),
            ("forwardagent", "no"),
            ("forwardx11", "no"),
            ("forwardx11trusted", "no"),
            ("clearallforwardings", "yes"),
            ("controlmaster", "false"),
            ("permitlocalcommand", "no"),
            ("tunnel", "false"),
            ("sessiontype", "default"),
            ("stdinnull", "no"),
            ("forkafterauthentication", "no"),
            ("hostkeyalias", alias),
            ("userknownhostsfile", pins),
        ];
        for (key, value) in wanted {
            if self.0.get(key).map(Vec::as_slice) != Some(&[value.to_owned()]) {
                return Err(unsafe_config(key));
            }
        }
        if self.get("gssapikeyexchange").is_some_and(|v| v != "no") {
            return Err(unsafe_config("gssapikeyexchange"));
        }
        for key in [
            "knownhostscommand",
            "controlpath",
            "localforward",
            "remoteforward",
            "dynamicforward",
            "localcommand",
            "remotecommand",
        ] {
            if self
                .0
                .get(key)
                .is_some_and(|v| v.iter().any(|v| v != "none"))
            {
                return Err(unsafe_config(key));
            }
        }
        Ok(())
    }
}

fn unsafe_config(key: &str) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        format!("your SSH config sets {key} in a way Ovra cannot connect safely with"),
    )
}

#[cfg(test)]
mod tests {
    use super::Effective;

    const SAFE: &str = "\
user dev
hostname build.example.org
port 22
stricthostkeychecking true
updatehostkeys false
batchmode yes
checkhostip no
requesttty false
globalknownhostsfile none
userknownhostsfile /d/ssh/h1/known_hosts
verifyhostkeydns false
gssapidelegatecredentials no
gssapikeyexchange no
forwardagent no
forwardx11 no
forwardx11trusted no
clearallforwardings yes
controlmaster false
permitlocalcommand no
tunnel false
sessiontype default
stdinnull no
forkafterauthentication no
hostkeyalias ovra-h1
sendenv LANG
";

    #[test]
    fn a_safe_config_passes_and_each_unsafe_value_is_named() {
        let pins = "/d/ssh/h1/known_hosts";
        assert!(Effective::parse(SAFE).check(pins, "ovra-h1").is_ok());
        for (from, to) in [
            ("forwardagent no", "forwardagent yes"),
            (
                "stricthostkeychecking true",
                "stricthostkeychecking accept-new",
            ),
            (
                "userknownhostsfile /d/ssh/h1/known_hosts",
                "userknownhostsfile /d/ssh/h1/known_hosts ~/.ssh/known_hosts",
            ),
            ("hostkeyalias ovra-h1", "hostkeyalias build"),
            ("gssapikeyexchange no", "gssapikeyexchange yes"),
            ("sessiontype default", "sessiontype none"),
            ("updatehostkeys false", "updatehostkeys true"),
            ("batchmode yes", "batchmode no"),
        ] {
            let text = SAFE.replace(from, to);
            let key = from.split(' ').next().unwrap_or_default();
            let err = Effective::parse(&text).check(pins, "ovra-h1").err();
            assert!(err.is_some_and(|e| e.message.contains(key)), "{to}");
        }
        for extra in [
            "knownhostscommand /bin/evil",
            "controlpath /tmp/master",
            "localforward 9000 [x]:22",
            "remoteforward 9000 [x]:22",
            "dynamicforward 1080",
        ] {
            let text = format!("{SAFE}{extra}\n");
            assert!(
                Effective::parse(&text).check(pins, "ovra-h1").is_err(),
                "{extra}"
            );
        }
        // `ssh -G` prints the pin path unquoted, `%%` and `\\` undone; a
        // path with a space is compared whole.
        let spaced = SAFE.replace("/d/ssh/h1/known_hosts", "/d a/x%y/known_hosts");
        assert!(
            Effective::parse(&spaced)
                .check("/d a/x%y/known_hosts", "ovra-h1")
                .is_ok()
        );
        assert!(Effective::parse(&spaced).check("/d", "ovra-h1").is_err());
        let missing = SAFE.replace("forwardx11 no\n", "");
        assert!(Effective::parse(&missing).check(pins, "ovra-h1").is_err());
    }

    #[test]
    fn keys_are_looked_up_under_the_name_ssh_checks() {
        let text =
            "hostname h.example\nport 2222\nuserknownhostsfile /a /b\nglobalknownhostsfile none\n";
        let spaced = Effective::parse("userknownhostsfile /tmp/a b/kh /c\n");
        assert_eq!(spaced.known_files(), ["/tmp/a", "/c"]);
        let effective = Effective::parse(text);
        assert_eq!(effective.key_name().as_deref(), Some("[h.example]:2222"));
        assert_eq!(effective.known_files(), ["/a", "/b"]);
        let aliased = Effective::parse("hostname h\nport 22\nhostkeyalias box\n");
        assert_eq!(aliased.key_name().as_deref(), Some("box"));
        assert_eq!(
            Effective::parse("hostname h\nport 22\n")
                .key_name()
                .as_deref(),
            Some("h")
        );
    }
}
