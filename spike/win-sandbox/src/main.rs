//! Throwaway spike for task 5.A: what an AppContainer process and a separate
//! local user can reach on a real Windows machine. Not part of the product.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("probe") => probe(&args[2..]),
        Some("host") => host(),
        _ => eprintln!("usage: spike host | spike probe <port> <secret> <home> <granted> <sock>"),
    }
}

fn line(name: &str, result: Result<String, String>) {
    match result {
        Ok(v) => println!("PROBE {name}: OK {v}"),
        Err(e) => println!("PROBE {name}: DENIED {e}"),
    }
}

fn probe(a: &[String]) {
    let port: u16 = a[0].parse().unwrap();
    let secret = PathBuf::from(&a[1]);
    let home = PathBuf::from(&a[2]);
    let granted = PathBuf::from(&a[3]);
    let sock = PathBuf::from(&a[4]);
    let t = Duration::from_secs(3);
    line("whoami", Ok(std::env::var("USERNAME").unwrap_or_default()));
    line(
        "tcp_loopback_host",
        TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), t)
            .map(|mut s| {
                let mut b = [0u8; 5];
                let _ = s.read(&mut b);
                String::from_utf8_lossy(&b).into_owned()
            })
            .map_err(|e| e.to_string()),
    );
    line(
        "tcp_internet_1.1.1.1",
        TcpStream::connect_timeout(&SocketAddr::from(([1, 1, 1, 1], 443)), t)
            .map(|_| "connected".into())
            .map_err(|e| e.to_string()),
    );
    line(
        "dns_example.com",
        ("example.com", 443)
            .to_socket_addrs()
            .map(|mut i| format!("{:?}", i.next()))
            .map_err(|e| e.to_string()),
    );
    line(
        "self_loopback",
        (|| {
            let l = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind {e}"))?;
            let addr = l.local_addr().unwrap();
            let th = std::thread::spawn(move || l.accept().map(|(mut s, _)| s.write_all(b"self!")));
            let mut c = TcpStream::connect_timeout(&addr, t).map_err(|e| format!("connect {e}"))?;
            let mut b = [0u8; 5];
            c.read_exact(&mut b).map_err(|e| format!("read {e}"))?;
            let _ = th.join();
            Ok(String::from_utf8_lossy(&b).into_owned())
        })(),
    );
    line("read_secret", std::fs::read_to_string(&secret).map_err(|e| e.to_string()));
    line(
        "list_home",
        std::fs::read_dir(&home)
            .map(|d| format!("{} entries", d.count()))
            .map_err(|e| e.to_string()),
    );
    line(
        "read_hosts",
        std::fs::read(r"C:\Windows\System32\drivers\etc\hosts")
            .map(|b| format!("{} bytes", b.len()))
            .map_err(|e| e.to_string()),
    );
    line(
        "write_granted",
        std::fs::write(granted.join("w.txt"), "x")
            .map(|_| "written".into())
            .map_err(|e| e.to_string()),
    );
    line(
        "write_temp_env",
        std::fs::write(std::env::temp_dir().join("ovra-spike-w.txt"), "x")
            .map(|_| format!("{}", std::env::temp_dir().display()))
            .map_err(|e| e.to_string()),
    );
    line("unix_connect", unix::connect(&sock));
    line(
        "git_version",
        Command::new("git")
            .arg("--version")
            .output()
            .map_err(|e| e.to_string())
            .and_then(|o| {
                if o.status.success() {
                    Ok(String::from_utf8_lossy(&o.stdout).trim().to_owned())
                } else {
                    Err(format!("{} {}", o.status, String::from_utf8_lossy(&o.stderr).trim()))
                }
            }),
    );
    probe2(&granted);
    line(
        "node_version",
        Command::new("node")
            .arg("--version")
            .output()
            .map_err(|e| e.to_string())
            .and_then(|o| {
                if o.status.success() {
                    Ok(String::from_utf8_lossy(&o.stdout).trim().to_owned())
                } else {
                    Err(format!("{} {}", o.status, String::from_utf8_lossy(&o.stderr).trim()))
                }
            }),
    );
}

fn run_piped(program: &str, args: &[&str], cwd: &Path) -> Result<String, String> {
    use std::process::Stdio;
    let out = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    let text = format!(
        "{} | {}",
        String::from_utf8_lossy(&out.stdout).trim().replace('\n', " / "),
        String::from_utf8_lossy(&out.stderr).trim().replace('\n', " / ")
    );
    if out.status.success() { Ok(text) } else { Err(format!("{} {text}", out.status)) }
}

fn probe2(granted: &Path) {
    for name in ["NUL", r"\\.\NUL", r"\\?\GLOBALROOT\Device\Null"] {
        for write in [false, true] {
            let r = std::fs::OpenOptions::new().read(!write).write(write).open(name);
            line(&format!("open {name} write={write}"), r.map(|_| "opened".into()).map_err(|e| e.to_string()));
        }
    }
    let git = r"C:\Program Files\Git\cmd\git.exe";
    let bash = r"C:\Program Files\Git\bin\bash.exe";
    line("git_piped", run_piped(git, &["--version"], granted));
    let repo = granted.join("repo");
    let _ = std::fs::create_dir_all(&repo);
    line("git_init", run_piped(git, &["init", "-q"], &repo));
    let _ = std::fs::write(repo.join("a.txt"), "a");
    line("git_status", run_piped(git, &["status", "--short"], &repo));
    line("git_commit", run_piped(git, &["-c", "user.name=x", "-c", "user.email=x@x", "commit", "-qam", "x", "--allow-empty"], &repo));
    line("bash_echo", run_piped(bash, &["-c", "echo hi; echo w > b.txt && cat b.txt; ls / | head -3; uname -a"], granted));
    line("bash_nul", run_piped(bash, &["-c", "echo x > /dev/null && echo devnull-ok"], granted));
    line("cmd_nul", run_piped("cmd.exe", &["/c", "echo x > NUL && echo nul-ok"], granted));
    line("powershell", run_piped("powershell.exe", &["-NoProfile", "-Command", "Write-Output ps-ok"], granted));
    let prot = granted.join("prot");
    line("prot_read", std::fs::read_to_string(prot.join("file.txt")).map_err(|e| e.to_string()));
    line("prot_write_existing", std::fs::write(prot.join("file.txt"), "x").map(|_| "written".into()).map_err(|e| e.to_string()));
    line("prot_create", std::fs::write(prot.join("new.txt"), "x").map(|_| "created".into()).map_err(|e| e.to_string()));
    line("prot_mkdir", std::fs::create_dir(prot.join("sub")).map(|_| "made".into()).map_err(|e| e.to_string()));
    line("prot_delete_file", std::fs::remove_file(prot.join("file.txt")).map(|_| "deleted".into()).map_err(|e| e.to_string()));
    line("prot_rename_dir", std::fs::rename(&prot, granted.join("prot2")).map(|_| "renamed".into()).map_err(|e| e.to_string()));
    line("prot_rename_file_out", std::fs::rename(prot.join("file.txt"), granted.join("moved.txt")).map(|_| "moved".into()).map_err(|e| e.to_string()));
    line("prot_icacls_remove_deny", run_piped("icacls.exe", &[prot.to_str().unwrap(), "/remove:d", "*S-1-15-2-1"], granted));
    line("prot_icacls_reset", run_piped("icacls.exe", &[prot.to_str().unwrap(), "/reset"], granted));
    let own = granted.join("own.txt");
    let _ = std::fs::write(&own, "x");
    line("own_icacls_grant", run_piped("icacls.exe", &[own.to_str().unwrap(), "/grant", "Everyone:F"], granted));
    line("own_takeown", run_piped("takeown.exe", &["/f", prot.to_str().unwrap()], granted));
    line("junction_to_home", run_piped("cmd.exe", &["/c", "mklink /J j C:\\Users && dir j"], granted));
    line("delete_granted_dir_child_tree", run_piped("cmd.exe", &["/c", "rd /s /q prot"], granted));
    line("prot_still_there", std::fs::read_to_string(prot.join("file.txt")).map_err(|e| e.to_string()));
}

/// Runs the listeners, writes the files and spawns the probe three ways.
fn host() {
    let home = PathBuf::from(std::env::var("USERPROFILE").unwrap());
    let secret = home.join(".ovra-spike-secret");
    std::fs::write(&secret, "TOP-SECRET").unwrap();
    let granted = PathBuf::from(r"C:\ovra-spike");
    let _ = std::fs::create_dir_all(&granted);
    let sock = granted.join("s.sock");
    let _ = std::fs::remove_file(&sock);

    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut s = s;
            let _ = s.write_all(b"host!");
        }
    });
    unix::serve(&sock);
    std::fs::write(granted.join("port.txt"), port.to_string()).unwrap();

    let exe = granted.join("spike.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
    let probe_args = |g: &Path| {
        vec![
            "probe".to_owned(),
            port.to_string(),
            secret.display().to_string(),
            home.display().to_string(),
            g.display().to_string(),
            sock.display().to_string(),
        ]
    };

    println!("=== baseline (unsandboxed) ===");
    let out = Command::new(&exe).args(probe_args(&granted)).output().unwrap();
    print!("{}", String::from_utf8_lossy(&out.stdout));

    println!("=== AppContainer, no capabilities ===");
    appcontainer::run(&exe, &probe_args(&granted), &granted);

    // Keep listeners up for the separate-user step, run by the workflow.
    if std::env::var("SPIKE_HOLD").is_ok() {
        std::fs::write(granted.join("ready.txt"), "1").unwrap();
        while !granted.join("done.txt").exists() {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

mod unix {
    use std::path::Path;
    use windows_sys::Win32::Networking::WinSock::*;

    #[repr(C)]
    struct SockaddrUn {
        family: u16,
        path: [u8; 108],
    }

    fn addr(p: &Path) -> SockaddrUn {
        let mut a = SockaddrUn { family: AF_UNIX, path: [0; 108] };
        let s = p.to_str().unwrap().as_bytes();
        a.path[..s.len()].copy_from_slice(s);
        a
    }

    fn init() {
        unsafe {
            let mut d = std::mem::zeroed();
            WSAStartup(0x0202, &mut d);
        }
    }

    pub fn serve(p: &Path) {
        init();
        unsafe {
            let s = socket(AF_UNIX as i32, SOCK_STREAM, 0);
            let a = addr(p);
            let r = bind(s, &a as *const _ as *const SOCKADDR, size_of::<SockaddrUn>() as i32);
            if r != 0 {
                println!("host unix bind failed {}", WSAGetLastError());
                return;
            }
            listen(s, 8);
            let s = s as usize;
            std::thread::spawn(move || loop {
                let c = accept(s as SOCKET, std::ptr::null_mut(), std::ptr::null_mut());
                if c == INVALID_SOCKET {
                    break;
                }
                send(c, b"unix!".as_ptr(), 5, 0);
                closesocket(c);
            });
        }
    }

    pub fn connect(p: &Path) -> Result<String, String> {
        init();
        unsafe {
            let s = socket(AF_UNIX as i32, SOCK_STREAM, 0);
            if s == INVALID_SOCKET {
                return Err(format!("socket {}", WSAGetLastError()));
            }
            let a = addr(p);
            let r = windows_sys::Win32::Networking::WinSock::connect(
                s,
                &a as *const _ as *const SOCKADDR,
                size_of::<SockaddrUn>() as i32,
            );
            if r != 0 {
                return Err(format!("connect {}", WSAGetLastError()));
            }
            let mut b = [0u8; 5];
            let n = recv(s, b.as_mut_ptr(), 5, 0);
            closesocket(s);
            Ok(String::from_utf8_lossy(&b[..n.max(0) as usize]).into_owned())
        }
    }
}

mod appcontainer {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::process::Command;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::Isolation::*;
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::System::Threading::*;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain([0]).collect()
    }

    pub fn run(exe: &Path, args: &[String], granted: &Path) {
        unsafe {
            let name = wide("ovra.spike");
            let mut sid: PSID = std::ptr::null_mut();
            let hr = CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                std::ptr::null(),
                0,
                &mut sid,
            );
            if hr != 0 {
                let hr2 = DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid);
                println!("profile exists ({hr:#x}), derive {hr2:#x}");
            }
            let mut ssid: *mut u16 = std::ptr::null_mut();
            ConvertSidToStringSidW(sid, &mut ssid);
            let len = (0..).take_while(|&i| *ssid.add(i) != 0).count();
            let sid_str = String::from_utf16_lossy(std::slice::from_raw_parts(ssid, len));
            println!("AppContainer SID {sid_str}");
            let g = Command::new("icacls")
                .arg(granted)
                .args(["/grant", &format!("*{sid_str}:(OI)(CI)M"), "/T"])
                .output()
                .unwrap();
            println!("icacls: {}", String::from_utf8_lossy(&g.stdout).trim());
            let prot = granted.join("prot");
            std::fs::create_dir_all(&prot).unwrap();
            std::fs::write(prot.join("file.txt"), "protected").unwrap();
            let d = Command::new("icacls")
                .arg(&prot)
                .args(["/deny", &format!("*{sid_str}:(OI)(CI)(W,D,DC,WDAC,WO)")])
                .output()
                .unwrap();
            println!("icacls deny: {} {}", String::from_utf8_lossy(&d.stdout).trim(), String::from_utf8_lossy(&d.stderr).trim());
            let show = Command::new("icacls").arg(&prot).output().unwrap();
            println!("{}", String::from_utf8_lossy(&show.stdout));

            let caps = SECURITY_CAPABILITIES {
                AppContainerSid: sid,
                Capabilities: std::ptr::null_mut(),
                CapabilityCount: 0,
                Reserved: 0,
            };
            let mut size = 0usize;
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
            let mut buf = vec![0u8; size];
            let list = buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
            InitializeProcThreadAttributeList(list, 1, 0, &mut size);
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                &caps as *const _ as *const _,
                size_of::<SECURITY_CAPABILITIES>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            let out = granted.join("ac-out.txt");
            let _ = std::fs::remove_file(&out);
            // The probe's stdout goes through cmd's redirection into a granted file.
            let mut cmd = format!("cmd.exe /c \"\"{}\"", exe.display());
            for a in args {
                cmd.push_str(&format!(" \"{a}\""));
            }
            cmd.push_str(&format!(" > \"{}\" 2>&1\"", out.display()));
            let mut cmdw = wide(&cmd);
            let mut si: STARTUPINFOEXW = std::mem::zeroed();
            si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            si.lpAttributeList = list;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(
                std::ptr::null(),
                cmdw.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT,
                std::ptr::null(),
                wide(&granted.display().to_string()).as_ptr(),
                &si.StartupInfo,
                &mut pi,
            );
            if ok == 0 {
                println!("CreateProcessW failed {}", GetLastError());
                return;
            }
            let w = WaitForSingleObject(pi.hProcess, 60_000);
            let mut code = 0u32;
            GetExitCodeProcess(pi.hProcess, &mut code);
            println!("wait {w} exit {code}");
            match std::fs::read_to_string(&out) {
                Ok(s) => print!("{s}"),
                Err(e) => println!("no output: {e}"),
            }
        }
    }
}
