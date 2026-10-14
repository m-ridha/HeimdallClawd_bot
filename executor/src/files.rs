use crate::protocol::LIMIT;
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
    if path.is_empty()
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
    pub fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let (dir, name) = self.parent(path, false)?;
        let file = open_at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("Only regular files can be read"));
        }
        let mut bytes = Vec::new();
        file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
        if std::str::from_utf8(&bytes).is_err() && bytes.len() <= LIMIT {
            return Err(io::Error::other("File is not UTF-8 text"));
        }
        Ok(bytes)
    }
    pub fn write(&self, path: &str, text: &str) -> io::Result<()> {
        if text.len() > LIMIT {
            return Err(io::Error::other("Write exceeds 64 KiB"));
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
            let metadata = unsafe { metadata.assume_init() };
            if metadata.st_mode & libc::S_IFMT != libc::S_IFREG {
                return Err(io::Error::other("Destination must be a regular file"));
            }
        } else if io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT) {
            return Err(io::Error::last_os_error());
        }
        let temp = CString::new(format!(".heimdall-{}", uuid::Uuid::new_v4())).unwrap();
        let operation = (|| {
            let mut file = open_at(
                &dir,
                &temp,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
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
        unsafe {
            libc::unlinkat(dir.as_raw_fd(), temp.as_ptr(), 0);
        }
        operation
    }
}
fn open_at(dir: &File, name: &CString, flags: i32, mode: u32) -> io::Result<File> {
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
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
