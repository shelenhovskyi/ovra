//! Spike (branch only): can WFP filters on the sandbox user close loopback
//! while the netproxy's port and the sandbox's own loopback stay usable?

#![cfg(windows)]
#![allow(clippy::unwrap_used, unsafe_code, clippy::undocumented_unsafe_blocks)]

use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::*;
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{GetSecurityDescriptorLength, PSECURITY_DESCRIPTOR};

const PROBE: &str = env!("CARGO_BIN_EXE_ovra-sandbox-probe");

fn text(path: &Path) -> String {
    path.display().to_string()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

struct Sd(PSECURITY_DESCRIPTOR, FWP_BYTE_BLOB);

fn sd_for(sid: &str) -> Box<Sd> {
    let sddl = wide(&format!("O:LSD:(A;;CC;;;{sid})"));
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let mut len = 0u32;
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut sd, &mut len)
    };
    assert!(ok != 0);
    let len = unsafe { GetSecurityDescriptorLength(sd) };
    Box::new(Sd(
        sd,
        FWP_BYTE_BLOB {
            size: len,
            data: sd.cast(),
        },
    ))
}

impl Drop for Sd {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

fn open() -> HANDLE {
    let session = FWPM_SESSION0 {
        flags: FWPM_SESSION_FLAG_DYNAMIC,
        ..Default::default()
    };
    let mut engine: HANDLE = std::ptr::null_mut();
    let err = unsafe {
        FwpmEngineOpen0(
            std::ptr::null(),
            10,
            std::ptr::null(),
            &session,
            &mut engine,
        )
    };
    assert_eq!(err, 0, "FwpmEngineOpen0");
    engine
}

enum Cond<'a> {
    Sd(windows_sys::core::GUID, &'a Sd),
    Loopback,
    Port(windows_sys::core::GUID, u16),
}

fn add(
    engine: HANDLE,
    layer: windows_sys::core::GUID,
    weight: u8,
    action: FWP_ACTION_TYPE,
    conds: &[Cond],
) {
    let mut built: Vec<FWPM_FILTER_CONDITION0> = conds
        .iter()
        .map(|c| {
            let mut f = FWPM_FILTER_CONDITION0::default();
            match c {
                Cond::Sd(key, sd) => {
                    f.fieldKey = *key;
                    f.matchType = FWP_MATCH_EQUAL;
                    f.conditionValue.r#type = FWP_SECURITY_DESCRIPTOR_TYPE;
                    f.conditionValue.Anonymous.sd = (&raw const sd.1).cast_mut();
                }
                Cond::Loopback => {
                    f.fieldKey = FWPM_CONDITION_FLAGS;
                    f.matchType = FWP_MATCH_FLAGS_ALL_SET;
                    f.conditionValue.r#type = FWP_UINT32;
                    f.conditionValue.Anonymous.uint32 = FWP_CONDITION_FLAG_IS_LOOPBACK;
                }
                Cond::Port(key, port) => {
                    f.fieldKey = *key;
                    f.matchType = FWP_MATCH_EQUAL;
                    f.conditionValue.r#type = FWP_UINT16;
                    f.conditionValue.Anonymous.uint16 = *port;
                }
            }
            f
        })
        .collect();
    let name = wide("ovra wfp spike");
    let mut filter = FWPM_FILTER0::default();
    filter.displayData.name = name.as_ptr().cast_mut();
    filter.layerKey = layer;
    filter.weight.r#type = FWP_UINT8;
    filter.weight.Anonymous.uint8 = weight;
    filter.numFilterConditions = built.len() as u32;
    filter.filterCondition = built.as_mut_ptr();
    filter.action.r#type = action;
    let mut id = 0u64;
    let err = unsafe { FwpmFilterAdd0(engine, &filter, std::ptr::null_mut(), &mut id) };
    assert_eq!(err, 0, "FwpmFilterAdd0 {err:#x}");
}

fn sandbox_sid() -> String {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", "(New-Object System.Security.Principal.NTAccount('OvraSandbox')).Translate([System.Security.Principal.SecurityIdentifier]).Value"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
#[ignore = "spike: needs an administrator"]
fn loopback_filters_for_the_sandbox_user() {
    let root = std::env::temp_dir().join(format!("ovra-wfp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, work) = (root.join("d"), root.join("w"));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let out = Command::new(PROBE)
        .args(["--ovra-sandbox-setup", &text(&data)])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sid = sandbox_sid();
    println!("sandbox sid {sid}");
    let outside = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let (a, b) = (outside.local_addr().unwrap(), proxy.local_addr().unwrap());
    let probe_dir = text(Path::new(PROBE).parent().unwrap());
    let run = |label: &str| {
        let out = Command::new(PROBE)
            .args([
                "--ovra-sandbox-helper",
                "--sid",
                "S-1-5-21-1-2-3-4",
                "--data",
                &text(&data),
            ])
            .args([
                "--m",
                &text(&work),
                "--rx",
                &probe_dir,
                "--env",
                "SystemRoot",
                "--",
                PROBE,
            ])
            .args([
                "connect",
                &a.to_string(),
                "connect",
                &b.to_string(),
                "selfloop",
                "x",
            ])
            .current_dir(&work)
            .output()
            .unwrap();
        println!(
            "== {label} (outside {a}, proxy {b})\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let user = sd_for(&sid);
    run("no filters");

    let engine = open();
    add(
        engine,
        FWPM_LAYER_ALE_AUTH_CONNECT_V4,
        1,
        FWP_ACTION_BLOCK,
        &[Cond::Sd(FWPM_CONDITION_ALE_USER_ID, &user), Cond::Loopback],
    );
    add(
        engine,
        FWPM_LAYER_ALE_AUTH_CONNECT_V4,
        10,
        FWP_ACTION_PERMIT,
        &[
            Cond::Sd(FWPM_CONDITION_ALE_USER_ID, &user),
            Cond::Loopback,
            Cond::Port(FWPM_CONDITION_IP_REMOTE_PORT, b.port()),
        ],
    );
    run("connect layer: block sandbox loopback, permit proxy port");
    unsafe { FwpmEngineClose0(engine) };

    let engine = open();
    add(
        engine,
        FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
        1,
        FWP_ACTION_BLOCK,
        &[
            Cond::Sd(FWPM_CONDITION_ALE_REMOTE_USER_ID, &user),
            Cond::Loopback,
        ],
    );
    add(
        engine,
        FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
        10,
        FWP_ACTION_PERMIT,
        &[
            Cond::Sd(FWPM_CONDITION_ALE_REMOTE_USER_ID, &user),
            Cond::Loopback,
            Cond::Port(FWPM_CONDITION_IP_LOCAL_PORT, b.port()),
        ],
    );
    add(
        engine,
        FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
        12,
        FWP_ACTION_PERMIT,
        &[Cond::Sd(FWPM_CONDITION_ALE_USER_ID, &user), Cond::Loopback],
    );
    run("accept layer: block from sandbox remote user, permit proxy port and sandbox listeners");
    unsafe { FwpmEngineClose0(engine) };

    run("after close");
    let _ = Command::new(PROBE).arg("--ovra-sandbox-remove").output();
    let _ = std::fs::remove_dir_all(&root);
    drop((outside, proxy));
}
