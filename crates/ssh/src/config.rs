//! What `ssh -G` says the person's config resolves to, and the check that
//! the hardened command line won over it.

use std::collections::BTreeMap;

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
    pub(crate) fn known_files(&self) -> Vec<String> {
        ["userknownhostsfile", "globalknownhostsfile"]
            .iter()
            .filter_map(|key| self.get(key))
            .flat_map(|list| list.split(' ').map(str::to_owned))
            .filter(|file| !file.is_empty() && file != "none")
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Effective;

    #[test]
    fn keys_are_looked_up_under_the_name_ssh_checks() {
        let text =
            "hostname h.example\nport 2222\nuserknownhostsfile /a /b\nglobalknownhostsfile none\n";
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
