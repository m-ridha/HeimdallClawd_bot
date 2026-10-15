//! Maintained agent instructions are refreshed at startup; replaced copies are preserved.
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
const MAX_INSTRUCTIONS: u64 = 131072;

fn sync_one(workspace: &Path, name: &str, content: &str) -> io::Result<()> {
    let path = workspace.join(name);
    let previous = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
    {
        Ok(file) => {
            if !file.metadata()?.is_file() {
                return Err(io::Error::other("Instruction file must be regular"));
            }
            let mut bytes = Vec::new();
            file.take(MAX_INSTRUCTIONS + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_INSTRUCTIONS {
                return Err(io::Error::other(
                    "Instruction file exceeds 128 KiB; move custom notes to USER.md",
                ));
            }
            Some(bytes)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return Err(io::Error::other("Cannot safely read instruction file")),
    };
    if previous.as_deref() == Some(content.as_bytes()) {
        return Ok(());
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::other("Invalid system clock"))?
        .as_nanos();
    let suffix = format!("{}-{nonce}", std::process::id());
    if let Some(previous) = previous {
        let backup = workspace.join(format!(".{name}.saved-{suffix}"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(backup)?;
        file.write_all(&previous)?;
        file.sync_all()?;
    }
    let temp = workspace.join(format!(".{name}.tmp-{suffix}"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let result = file
        .write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temp, &path));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

/// Refreshes managed prompt files. USER.md and other user notes are never changed.
pub fn sync(workspace: &Path) -> io::Result<()> {
    sync_one(
        workspace,
        "TOOLS.md",
        include_str!("../../deploy/bot/TOOLS.md"),
    )?;
    sync_one(
        workspace,
        "IDENTITY.md",
        include_str!("../../deploy/bot/IDENTITY.md"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_preserves_old_copies_and_user_notes_without_repeated_backups() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("TOOLS.md"), "old customized instructions").unwrap();
        fs::write(dir.path().join("USER.md"), "operator notes").unwrap();
        sync(dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("TOOLS.md")).unwrap(),
            include_str!("../../deploy/bot/TOOLS.md")
        );
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        let backup = entries
            .iter()
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".TOOLS.md.saved-")
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(backup).unwrap(),
            "old customized instructions"
        );
        sync(dir.path()).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), entries.len());
        assert_eq!(
            fs::read_to_string(dir.path().join("USER.md")).unwrap(),
            "operator notes"
        );
    }
    #[test]
    fn instruction_symlinks_are_rejected_without_touching_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, "protected").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("TOOLS.md")).unwrap();
        assert!(sync(dir.path()).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "protected");
    }
    #[test]
    fn oversized_notes_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TOOLS.md");
        fs::write(&path, vec![b'x'; MAX_INSTRUCTIONS as usize + 1]).unwrap();
        assert!(sync(dir.path()).is_err());
        assert_eq!(fs::metadata(path).unwrap().len(), MAX_INSTRUCTIONS + 1);
    }
}
