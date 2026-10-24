static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) const LIMIT: usize = 49_152;
const TEMP_ATTEMPTS: usize = 100;
use std::{
    ffi::CString,
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::OpenOptionsExt,
    },
    path::{Component, Path},
};

pub fn validate_path(path: &str) -> io::Result<()> {
    if path.len() > 1024
        || path.is_empty()
        || path.contains('\0')
        || Path::new(path)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Relative workspace paths without traversal are required",
        ));
    }
    Ok(())
}
pub struct Workspace {
    root: File,
}
impl Workspace {
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            root: File::options()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(path)?,
        })
    }
    fn parent(&self, path: &str, create: bool) -> io::Result<(File, CString)> {
        validate_path(path)?;
        let mut directory = self.root.try_clone()?;
        let parts: Vec<_> = Path::new(path).components().collect();
        for part in &parts[..parts.len() - 1] {
            let name = CString::new(part.as_os_str().as_encoded_bytes())?;
            if create {
                // SAFETY: valid directory descriptor and NUL-terminated component.
                let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
                if result < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                    return Err(io::Error::last_os_error());
                }
            }
            directory = open_at(&directory, &name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        }
        Ok((
            directory,
            CString::new(parts.last().unwrap().as_os_str().as_encoded_bytes())?,
        ))
    }
    pub fn read_binary(&self, path: &str) -> io::Result<Vec<u8>> {
        let (dir, name) = self.parent(path, false)?;
        let file = open_at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("Only regular files can be read"));
        }
        let mut bytes = Vec::new();
        file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > LIMIT {
            return Err(io::Error::other("Transfer exceeds 48 KiB"));
        }
        Ok(bytes)
    }
    pub fn write_binary(&self, path: &str, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > LIMIT {
            return Err(io::Error::other("Transfer exceeds 48 KiB"));
        }
        let (dir, name) = self.parent(path, true)?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstatat initializes the provided stat on success.
        let result = unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            // SAFETY: successful fstatat initialized metadata above.
            let metadata = unsafe { metadata.assume_init() };
            if metadata.st_mode & libc::S_IFMT != libc::S_IFREG {
                return Err(io::Error::other("Destination must be a regular file"));
            }
        } else if io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT) {
            return Err(io::Error::last_os_error());
        }
        let mut attempts = 0;
        let (temp, mut file) = loop {
            if attempts == TEMP_ATTEMPTS {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Workspace temporary-file retries exhausted",
                ));
            }
            attempts += 1;
            let temp = CString::new(format!(
                ".heimdall-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ))
            .unwrap();
            match open_at(
                &dir,
                &temp,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            ) {
                Ok(file) => break (temp, file),
                Err(error) if error.raw_os_error() == Some(libc::EEXIST) => continue,
                Err(error) => return Err(error),
            }
        };
        let operation = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            // SAFETY: both descriptors and C strings remain live; rename replaces
            // the directory entry without following a destination symlink.
            // Atomic replacement never follows a concurrently substituted destination symlink.
            if unsafe {
                libc::renameat(
                    dir.as_raw_fd(),
                    temp.as_ptr(),
                    dir.as_raw_fd(),
                    name.as_ptr(),
                )
            } < 0
            {
                return Err(io::Error::last_os_error());
            }
            dir.sync_all()?;
            Ok(())
        })();
        // SAFETY: live directory descriptor and valid temporary C string.
        unsafe {
            libc::unlinkat(dir.as_raw_fd(), temp.as_ptr(), 0);
        }
        operation
    }
}
fn open_at(dir: &File, name: &CString, flags: i32, mode: u32) -> io::Result<File> {
    // SAFETY: live directory descriptor and NUL-terminated name; flags reject symlinks.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: successful openat returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_retry_temporary_name_collisions_without_touching_stale_entries() {
        if std::env::var_os("HEIMDALL_WORKSPACE_COLLISION_TEST").is_none() {
            // Isolate NEXT from workspace writes running in parallel tests.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "workspace::tests::writes_retry_temporary_name_collisions_without_touching_stale_entries",
                ])
                .env("HEIMDALL_WORKSPACE_COLLISION_TEST", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "collision test failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let root = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let prefix = format!(".heimdall-{}", std::process::id());
        let stale = root.path().join(format!("{prefix}-0"));
        let link = root.path().join(format!("{prefix}-1"));
        let directory = root.path().join(format!("{prefix}-2"));
        std::fs::write(&stale, b"stale content").unwrap();
        std::os::unix::fs::symlink(&stale, &link).unwrap();
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(root.path().join("file"), b"old content").unwrap();

        workspace.write_binary("file", b"replacement").unwrap();

        assert_eq!(workspace.read_binary("file").unwrap(), b"replacement");
        assert_eq!(std::fs::read(&stale).unwrap(), b"stale content");
        assert_eq!(std::fs::read_link(&link).unwrap(), stale);
        assert!(directory.is_dir());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 4);

        // Bound collision work even if the directory contains many stale entries.
        for counter in 4..104 {
            std::fs::write(root.path().join(format!("{prefix}-{counter}")), b"retained").unwrap();
        }
        let error = workspace.write_binary("blocked", b"content").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(!root.path().join("blocked").exists());
        assert_eq!(
            std::fs::read(root.path().join(format!("{prefix}-4"))).unwrap(),
            b"retained"
        );
    }

    #[test]
    fn temporary_creation_propagates_errors_other_than_collisions() {
        let root = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        std::fs::remove_dir(root.path()).unwrap();

        let error = workspace.write_binary("file", b"content").unwrap_err();

        assert_eq!(error.raw_os_error(), Some(libc::ENOENT));
    }

    #[test]
    fn bounded_local_transfers_reject_traversal_symlinks_and_special_files() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        workspace.write_binary("nested/file", b"hello").unwrap();
        assert_eq!(workspace.read_binary("nested/file").unwrap(), b"hello");
        workspace
            .write_binary("nested/file", b"replacement")
            .unwrap();
        assert_eq!(
            workspace.read_binary("nested/file").unwrap(),
            b"replacement"
        );
        for path in [
            "",
            "../escape",
            "/absolute",
            "nested/../escape",
            &"a".repeat(1025),
        ] {
            assert!(workspace.read_binary(path).is_err());
            assert!(workspace.write_binary(path, b"x").is_err());
        }
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        assert!(workspace.write_binary("link/file", b"x").is_err());
        std::os::unix::fs::symlink("nested/file", root.path().join("file-link")).unwrap();
        assert!(workspace.read_binary("file-link").is_err());
        assert!(workspace.write_binary("file-link", b"x").is_err());
        assert!(workspace.write_binary("large", &vec![0; 49_153]).is_err());
        std::fs::write(root.path().join("large"), vec![0; 49_153]).unwrap();
        assert!(workspace.read_binary("large").is_err());
        assert!(workspace.read_binary("nested").is_err());
        assert!(!outside.path().join("file").exists());
    }
}
