use std::io;
/// Two admitted helpers leave room for the supervisor within the 250 MB container.
pub const HELPER_ADDRESS_SPACE: u64 = 96 * 1024 * 1024;
#[cfg(target_os = "linux")]
pub fn restrict() -> io::Result<()> {
    use seccompiler::{
        BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
        SeccompRule, TargetArch,
    };
    use std::collections::BTreeMap;
    // Helpers start as an unprivileged UID in their own process group.
    if unsafe { libc::geteuid() } != 64173 {
        return Err(io::Error::other("Execution UID is not isolated"));
    }
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for (resource, value) in [
        (libc::RLIMIT_CPU, 30),
        (libc::RLIMIT_AS, HELPER_ADDRESS_SPACE),
        (libc::RLIMIT_NOFILE, 64),
        (libc::RLIMIT_NPROC, 64),
        (libc::RLIMIT_FSIZE, 32 * 1024 * 1024),
    ] {
        let limit = libc::rlimit {
            rlim_cur: value,
            rlim_max: value,
        };
        if unsafe { libc::setrlimit(resource, &limit) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let allowed = [
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_close,
        libc::SYS_fstat,
        libc::SYS_newfstatat,
        libc::SYS_statx,
        libc::SYS_lseek,
        libc::SYS_mmap,
        libc::SYS_mprotect,
        libc::SYS_munmap,
        libc::SYS_brk,
        libc::SYS_madvise,
        // Ordinary allocators, timestamp-preserving copy/archive tools and file locks.
        libc::SYS_mremap,
        libc::SYS_utimensat,
        libc::SYS_statfs,
        libc::SYS_fstatfs,
        libc::SYS_flock,
        // cp -p reads/writes POSIX ACLs through file descriptors, even for files
        // without an ACL. Kernel UID/DAC checks still govern metadata changes.
        libc::SYS_fgetxattr,
        libc::SYS_fsetxattr,
        // Never executed: a stacked filter below returns ENOSYS for glibc fallback.
        libc::SYS_clone3,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_ioctl,
        libc::SYS_pread64,
        libc::SYS_pwrite64,
        libc::SYS_readv,
        libc::SYS_writev,
        libc::SYS_access,
        libc::SYS_openat,
        libc::SYS_open,
        // GNU tar's rmtcreat uses the x86_64 creat syscall, equivalent to
        // open(O_WRONLY | O_CREAT | O_TRUNC); ordinary UID/DAC checks apply.
        libc::SYS_creat,
        libc::SYS_stat,
        libc::SYS_lstat,
        libc::SYS_readlink,
        libc::SYS_readlinkat,
        libc::SYS_getcwd,
        libc::SYS_chdir,
        libc::SYS_fchdir,
        libc::SYS_getdents64,
        libc::SYS_getdents,
        libc::SYS_mkdir,
        libc::SYS_mkdirat,
        libc::SYS_rmdir,
        libc::SYS_unlink,
        libc::SYS_unlinkat,
        libc::SYS_rename,
        libc::SYS_renameat,
        libc::SYS_renameat2,
        libc::SYS_link,
        libc::SYS_linkat,
        libc::SYS_symlink,
        libc::SYS_symlinkat,
        libc::SYS_fsync,
        libc::SYS_fdatasync,
        libc::SYS_ftruncate,
        libc::SYS_truncate,
        libc::SYS_fchmod,
        libc::SYS_fchmodat,
        libc::SYS_chmod,
        libc::SYS_umask,
        libc::SYS_dup,
        libc::SYS_dup2,
        libc::SYS_dup3,
        libc::SYS_pipe,
        libc::SYS_pipe2,
        libc::SYS_fcntl,
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_fork,
        libc::SYS_vfork,
        libc::SYS_wait4,
        libc::SYS_waitid,
        libc::SYS_getpid,
        libc::SYS_getppid,
        libc::SYS_gettid,
        libc::SYS_getuid,
        libc::SYS_geteuid,
        libc::SYS_getgid,
        libc::SYS_getegid,
        libc::SYS_getgroups,
        libc::SYS_getpgrp,
        libc::SYS_getpgid,
        libc::SYS_getsid,
        libc::SYS_set_tid_address,
        libc::SYS_set_robust_list,
        libc::SYS_futex,
        libc::SYS_rseq,
        libc::SYS_arch_prctl,
        libc::SYS_clock_gettime,
        libc::SYS_clock_nanosleep,
        libc::SYS_nanosleep,
        libc::SYS_getrandom,
        libc::SYS_uname,
        libc::SYS_getrlimit,
        libc::SYS_prlimit64,
        libc::SYS_getrusage,
        libc::SYS_times,
        libc::SYS_sched_getaffinity,
        libc::SYS_sched_yield,
        libc::SYS_sysinfo,
        libc::SYS_poll,
        libc::SYS_ppoll,
        libc::SYS_select,
        libc::SYS_pselect6,
        libc::SYS_exit,
        libc::SYS_exit_group,
        libc::SYS_restart_syscall,
    ];
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> =
        allowed.into_iter().map(|n| (n, vec![])).collect();
    // Disallow namespace and parent-changing clone flags; clone3 remains denied.
    let forbidden = (libc::CLONE_NEWUSER
        | libc::CLONE_NEWNET
        | libc::CLONE_NEWPID
        | libc::CLONE_NEWNS
        | libc::CLONE_NEWUTS
        | libc::CLONE_NEWIPC
        | libc::CLONE_NEWCGROUP
        | libc::CLONE_PARENT) as u64;
    let condition = SeccompCondition::new(
        0,
        SeccompCmpArgLen::Qword,
        SeccompCmpOp::MaskedEq(forbidden),
        0,
    )
    .map_err(io::Error::other)?;
    rules.insert(
        libc::SYS_clone,
        vec![SeccompRule::new(vec![condition]).map_err(io::Error::other)?],
    );
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Errno(libc::EPERM as u32),
        SeccompAction::Allow,
        TargetArch::x86_64,
    )
    .map_err(io::Error::other)?;
    let program: BpfProgram = filter.try_into().map_err(io::Error::other)?;
    // clone3 remains unavailable, but glibc pthread/posix_spawn can fall back to
    // clone, whose namespace/parent-changing flags remain denied above.
    let fallback = SeccompFilter::new(
        BTreeMap::from([(libc::SYS_clone3, vec![])]),
        SeccompAction::Allow,
        SeccompAction::Errno(libc::ENOSYS as u32),
        TargetArch::x86_64,
    )
    .map_err(io::Error::other)?;
    let fallback: BpfProgram = fallback.try_into().map_err(io::Error::other)?;
    // Install the fallback first: the final allowlist denies seccomp/prctl,
    // so installing another filter after it would fail. ERRNO outranks ALLOW.
    seccompiler::apply_filter(&fallback).map_err(io::Error::other)?;
    seccompiler::apply_filter(&program).map_err(io::Error::other)
}
#[cfg(not(target_os = "linux"))]
pub fn restrict() -> io::Result<()> {
    Err(io::Error::other("Linux restrictions are required"))
}

#[cfg(target_os = "linux")]
pub fn probe() -> io::Result<()> {
    // A real pthread exercises glibc clone3 -> clone fallback under the filter.
    let thread = std::thread::Builder::new().spawn(|| 42)?;
    if thread
        .join()
        .map_err(|_| io::Error::other("Thread probe panicked"))?
        != 42
    {
        return Err(io::Error::other("Thread probe failed"));
    }
    if std::env::var_os("EXECUTOR_TOKEN").is_some() {
        return Err(io::Error::other("Control secret inherited"));
    }
    for syscall in [
        libc::SYS_socket,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_setsid,
        libc::SYS_setpgid,
        libc::SYS_unshare,
    ] {
        let result = unsafe { libc::syscall(syscall, 0, 0, 0, 0, 0, 0) };
        if result != -1 || io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
            return Err(io::Error::other("Forbidden syscall was not denied"));
        }
    }
    // SAFETY: null clone3 arguments cannot create a process; filter must return ENOSYS.
    if unsafe { libc::syscall(libc::SYS_clone3, 0, 0) } != -1
        || io::Error::last_os_error().raw_os_error() != Some(libc::ENOSYS)
    {
        return Err(io::Error::other("clone3 fallback policy failed"));
    }
    let parent = unsafe { libc::getppid() };
    if std::fs::read(format!("/proc/{parent}/environ")).is_ok()
        || std::fs::read("/run/heimdall-control/probe").is_ok()
    {
        return Err(io::Error::other("Control process is readable"));
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
pub fn probe() -> io::Result<()> {
    Err(io::Error::other("Linux probe required"))
}
