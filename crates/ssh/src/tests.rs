use std::path::Path;

use ovra_core::ssh::{DaemonPath, Destination, HostId};

use super::arguments;

fn destination() -> Destination {
    Destination::parse("dev@build").unwrap_or_else(|e| panic!("{e:?}"))
}

fn id() -> HostId {
    HostId::parse("h1").unwrap_or_else(|e| panic!("{e:?}"))
}

fn daemon() -> DaemonPath {
    DaemonPath::parse(Some("~/bin/ovra-daemon")).unwrap_or_else(|e| panic!("{e:?}"))
}

#[test]
#[cfg(unix)]
fn the_command_line_is_fixed() {
    let pins = Path::new("/Users/me/Library/Application Support/ovra/ssh/h1/known_hosts");
    let args = arguments(pins, &id(), &destination(), &daemon());
    insta::assert_json_snapshot!(args.unwrap_or_else(|e| panic!("{e:?}")));
}

#[test]
#[cfg(unix)]
fn the_pin_path_cannot_break_out_of_its_option() {
    let args = arguments(
        Path::new("/data/100%/known_hosts"),
        &id(),
        &destination(),
        &daemon(),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(args.contains(&"UserKnownHostsFile=\"/data/100%%/known_hosts\"".to_owned()));
    let args = arguments(
        Path::new("/data/a\\b/known_hosts"),
        &id(),
        &destination(),
        &daemon(),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(args.contains(&"UserKnownHostsFile=\"/data/a\\\\b/known_hosts\"".to_owned()));
    for bad in [
        "/data/a\"b/known_hosts",
        "/data/a\nb/known_hosts",
        "/data/${HOME}/known_hosts",
        "relative/known_hosts",
    ] {
        assert!(
            arguments(Path::new(bad), &id(), &destination(), &daemon()).is_err(),
            "{bad:?}"
        );
    }
}
