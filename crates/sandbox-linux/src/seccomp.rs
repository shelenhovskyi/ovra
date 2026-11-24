//! The seccomp filter the helper applies last: system calls an agent's
//! tools never need, which only widen the kernel an attacker can reach or
//! undo the sandbox. Everything else is allowed; the mounts, Landlock and
//! the namespaces are what limit files and the network.

use std::collections::BTreeMap;

use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch, sock_filter,
};

/// Refused with `EPERM`.
const DENIED: &[i64] = &[
    // Kernel keyrings: secrets other programs of the user keep there.
    libc::SYS_keyctl,
    libc::SYS_add_key,
    libc::SYS_request_key,
    // Large kernel surfaces no build needs.
    libc::SYS_bpf,
    libc::SYS_perf_event_open,
    libc::SYS_userfaultfd,
    libc::SYS_io_uring_setup,
    libc::SYS_io_uring_enter,
    libc::SYS_io_uring_register,
    // Changing the mounts or namespaces bubblewrap set up.
    libc::SYS_mount,
    libc::SYS_umount2,
    libc::SYS_pivot_root,
    libc::SYS_setns,
    libc::SYS_fsopen,
    libc::SYS_fsmount,
    libc::SYS_fsconfig,
    libc::SYS_fspick,
    libc::SYS_move_mount,
    libc::SYS_open_tree,
    libc::SYS_mount_setattr,
    // Opening files by handle walks around the mount namespace.
    libc::SYS_open_by_handle_at,
    libc::SYS_name_to_handle_at,
    // The machine itself.
    libc::SYS_kexec_load,
    libc::SYS_kexec_file_load,
    libc::SYS_init_module,
    libc::SYS_finit_module,
    libc::SYS_delete_module,
    libc::SYS_reboot,
    libc::SYS_swapon,
    libc::SYS_swapoff,
    libc::SYS_acct,
    libc::SYS_quotactl,
];

/// `ioctl` requests that type into the terminal or console.
const TIOCSTI: u64 = 0x5412;
const TIOCLINUX: u64 = 0x541C;

/// The bit that marks an x32 system call on x86_64. x32 numbers its calls
/// in its own way, so every x32 call is refused.
const X32: u32 = 0x4000_0000;

/// Installs the filters on this thread; they pass to everything it runs.
pub(crate) fn apply() -> Result<(), seccompiler::Error> {
    for program in programs()? {
        seccompiler::apply_filter(&program)?;
    }
    Ok(())
}

fn programs() -> Result<Vec<BpfProgram>, seccompiler::Error> {
    let arch = TargetArch::try_from(std::env::consts::ARCH)?;
    let mut denied: BTreeMap<i64, Vec<SeccompRule>> =
        DENIED.iter().map(|&nr| (nr, Vec::new())).collect();
    // A new user namespace would give the agent capabilities in it, and
    // with them kernel code an unprivileged process cannot otherwise reach.
    let new_user = || {
        SeccompCondition::new(
            0,
            SeccompCmpArgLen::Qword,
            SeccompCmpOp::MaskedEq(libc::CLONE_NEWUSER as u64),
            libc::CLONE_NEWUSER as u64,
        )
        .and_then(|c| SeccompRule::new(vec![c]))
    };
    denied.insert(libc::SYS_clone, vec![new_user()?]);
    denied.insert(libc::SYS_unshare, vec![new_user()?]);
    let ioctl = |request| {
        SeccompCondition::new(1, SeccompCmpArgLen::Dword, SeccompCmpOp::Eq, request)
            .and_then(|c| SeccompRule::new(vec![c]))
    };
    denied.insert(libc::SYS_ioctl, vec![ioctl(TIOCSTI)?, ioctl(TIOCLINUX)?]);
    let eperm = SeccompFilter::new(
        denied,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        arch,
    )?;
    // `clone3` passes its flags in memory, where a filter cannot look:
    // `ENOSYS` makes the C library fall back to `clone`, which is checked.
    let clone3 = BTreeMap::from([(libc::SYS_clone3, Vec::new())]);
    let enosys = SeccompFilter::new(
        clone3,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::ENOSYS as u32),
        arch,
    )?;
    let mut programs = vec![eperm.try_into()?, enosys.try_into()?];
    if arch == TargetArch::x86_64 {
        programs.push(no_x32());
    }
    Ok(programs)
}

/// On x86_64: `EPERM` for every call numbered `X32` or above. The filters
/// above check the architecture, which x32 shares with x86_64.
fn no_x32() -> BpfProgram {
    const LD_ABS: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
    const JGE: u16 = 0x05 | 0x30; // BPF_JMP | BPF_JGE | BPF_K
    const RET: u16 = 0x06; // BPF_RET | BPF_K
    const ALLOW: u32 = 0x7fff_0000;
    const ERRNO: u32 = 0x0005_0000;
    let op = |code, jt, jf, k| sock_filter { code, jt, jf, k };
    vec![
        // `seccomp_data.nr` is at offset 0.
        op(LD_ABS, 0, 0, 0),
        op(JGE, 0, 1, X32),
        op(RET, 0, 0, ERRNO | libc::EPERM as u32),
        op(RET, 0, 0, ALLOW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filters_compile_for_this_machine() {
        let programs = programs().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            programs.len(),
            2 + usize::from(cfg!(target_arch = "x86_64"))
        );
        assert!(programs.iter().all(|p| !p.is_empty()));
    }
}
