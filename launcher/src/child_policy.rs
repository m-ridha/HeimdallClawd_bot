//! Filesystem confinement for credential-bearing agent subprocess boundaries.
use std::os::unix::fs::DirBuilderExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, io, path::PathBuf, process::Command};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Copy)]
pub enum Scope {
    /// Scratch-only profile used to verify inherited child restrictions.
    Scratch,
    Skills,
    Codespaces,
}
/// Keeps the child's private temporary directory alive through group cleanup.
pub struct Session {
    path: PathBuf,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
impl Session {
    pub fn directory(&self) -> &std::path::Path {
        &self.path
    }
    pub fn new() -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "heimdall-child-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self { path })
    }
    pub fn configure(&self, command: &mut Command, scope: Scope) -> io::Result<()> {
        command.current_dir(&self.path).env("TMPDIR", &self.path);
        if matches!(scope, Scope::Scratch | Scope::Skills) {
            command.env("HOME", &self.path);
        }
        #[cfg(target_os = "linux")]
        {
            linux::configure(command, scope, &self.path)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = scope;
            Err(io::Error::other(
                "Child filesystem isolation requires Linux",
            ))
        }
    }
}
#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::ffi::CString;
    use std::os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    };
    // Landlock ABI 3 includes REFER and TRUNCATE. Older kernels fail closed.
    const READ: u64 = (1 << 0) | (1 << 2) | (1 << 3);
    const ALL: u64 = (1 << 15) - 1;
    // No native tool needs to create character/block devices, even in scratch.
    const WRITABLE: u64 = ALL & !((1 << 6) | (1 << 11));
    #[repr(C)]
    struct Ruleset {
        handled_access_fs: u64,
    }
    #[repr(C, packed)]
    struct Beneath {
        allowed_access: u64,
        parent_fd: i32,
    }
    fn fd(result: i64) -> io::Result<OwnedFd> {
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful open/syscall returns a new owned descriptor.
        Ok(unsafe { OwnedFd::from_raw_fd(result as i32) })
    }
    fn open(path: &str, trusted_system: bool) -> io::Result<OwnedFd> {
        if trusted_system {
            let path = CString::new(path)?;
            // SAFETY: valid terminated path; system library symlinks are trusted.
            return fd(unsafe { libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) } as i64);
        }
        // State paths must never follow a user-controlled symlink in any component.
        let mut current = open("/", true)?;
        for part in std::path::Path::new(path).components() {
            let part = match part {
                std::path::Component::Normal(part) => part,
                std::path::Component::RootDir => continue,
                _ => {
                    return Err(io::Error::other(
                        "Child policy requires absolute normalized paths",
                    ));
                }
            };
            use std::os::unix::ffi::OsStrExt;
            let name = CString::new(part.as_bytes())?;
            // SAFETY: current and name remain valid; O_NOFOLLOW pins the object.
            let next = fd(unsafe {
                libc::openat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_PATH | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            } as i64)?;
            // SAFETY: initialized stat points to writable storage; descriptor is live.
            // SAFETY: all-zero stat storage is valid before fstat initializes it.
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstat(next.as_raw_fd(), &mut stat) } != 0 {
                return Err(io::Error::last_os_error());
            }
            if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
                return Err(io::Error::other("Child policy rejects symlinks"));
            }
            current = next;
        }
        Ok(current)
    }
    fn add(
        ruleset: &OwnedFd,
        path: &str,
        access: u64,
        trusted: bool,
        optional: bool,
    ) -> io::Result<()> {
        let target = match open(path, trusted) {
            Ok(fd) => fd,
            Err(e) if optional && e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        // File rules cannot include directory-only rights.
        // SAFETY: all-zero stat storage is valid before fstat initializes it.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: live descriptor and writable stat.
        if unsafe { libc::fstat(target.as_raw_fd(), &mut stat) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let access = if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
            access
        } else {
            access & ((1 << 0) | (1 << 1) | (1 << 2) | (1 << 14))
        };
        let rule = Beneath {
            allowed_access: access,
            parent_fd: target.as_raw_fd(),
        };
        // SAFETY: kernel reads a correctly laid out path-beneath rule and live FDs.
        if unsafe {
            libc::syscall(
                libc::SYS_landlock_add_rule,
                ruleset.as_raw_fd(),
                1,
                &rule,
                0,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    pub fn configure(
        command: &mut Command,
        scope: Scope,
        temp: &std::path::Path,
    ) -> io::Result<()> {
        // SAFETY: version query uses null attributes as specified by Landlock UAPI.
        if unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<Ruleset>(),
                0,
                1,
            )
        } < 3
        {
            return Err(io::Error::other(
                "Landlock ABI 3 is required for child isolation",
            ));
        }
        let attr = Ruleset {
            handled_access_fs: ALL,
        };
        // SAFETY: kernel reads the UAPI struct; successful FD is exclusively owned.
        let ruleset = fd(unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &attr,
                std::mem::size_of::<Ruleset>(),
                0,
            )
        })?;
        for path in [
            "/usr",
            "/bin",
            "/lib",
            "/lib64",
            "/etc",
            "/dev/null",
            "/dev/urandom",
            "/dev/random",
        ] {
            add(
                &ruleset,
                path,
                if path.starts_with("/dev/") {
                    READ | (1 << 1)
                } else {
                    READ
                },
                true,
                true,
            )?;
        }
        add(
            &ruleset,
            temp.to_str()
                .ok_or_else(|| io::Error::other("Invalid child temporary path"))?,
            WRITABLE,
            false,
            false,
        )?;
        match scope {
            Scope::Scratch => {}
            Scope::Codespaces => add(&ruleset, "/data/zeroclaw/codespace", WRITABLE, false, false)?,
            Scope::Skills => {
                add(&ruleset, "/data/zeroclaw/config.toml", READ, false, false)?;
                add(
                    &ruleset,
                    "/data/zeroclaw/shared/skills",
                    WRITABLE,
                    false,
                    false,
                )?;
                // Only the registry subtree is writable. Landlock traversal does
                // not require READ on the parent (which would expose private data).
                add(
                    &ruleset,
                    "/data/zeroclaw/data/skills-registry",
                    WRITABLE,
                    false,
                    false,
                )?;
                for path in [
                    "/data/zeroclaw/agents/primary/workspace/skills",
                    "/data/zeroclaw/shared/plugins",
                ] {
                    add(&ruleset, path, READ, false, true)?;
                }
                add(
                    &ruleset,
                    "/data/zeroclaw/agents/primary/skills",
                    WRITABLE,
                    false,
                    true,
                )?;
            }
        }
        // Deny process-memory inspection even when the kernel permits same-UID ptrace.
        let insn = |code, jt, jf, k| libc::sock_filter { code, jt, jf, k };
        #[cfg(target_arch = "x86_64")]
        let audit_arch = 0xc000003e; // AUDIT_ARCH_X86_64
        #[cfg(target_arch = "aarch64")]
        let audit_arch = 0xc00000b7; // AUDIT_ARCH_AARCH64
        let load = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
        let equal = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16;
        let bits = (libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K) as u16;
        let ret = (libc::BPF_RET | libc::BPF_K) as u16;
        let denied = libc::SECCOMP_RET_ERRNO | libc::EPERM as u32;
        // seccomp_data stores syscall number at byte 0 and audit architecture at 4.
        let mut filter = vec![
            insn(load, 0, 0, 4),
            insn(equal, 1, 0, audit_arch),
            insn(ret, 0, 0, libc::SECCOMP_RET_KILL_PROCESS),
            insn(load, 0, 0, 0),
            insn(bits, 0, 1, 0x40000000), // Reject x32 syscall encodings.
            insn(ret, 0, 0, denied),
        ];
        for nr in [
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_kcmp,
            libc::SYS_pidfd_getfd,
        ] {
            filter.push(insn(equal, 0, 1, nr as u32));
            filter.push(insn(ret, 0, 0, denied));
        }
        filter.push(insn(ret, 0, 0, libc::SECCOMP_RET_ALLOW));
        // SAFETY: closure only invokes async-signal-safe syscalls, using a prepared FD.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                let program = libc::sock_fprog {
                    len: filter.len() as u16,
                    filter: filter.as_ptr() as *mut _,
                };
                if libc::prctl(libc::PR_SET_SECCOMP, 2, &program) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(())
    }
}
