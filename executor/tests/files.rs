use heimdall_executor::files::Workspace;
#[test]
fn files_are_bounded_and_relative() {
    let root = tempfile::tempdir().unwrap();
    let fs = Workspace::open(root.path()).unwrap();
    fs.write("a/note.txt", "hello").unwrap();
    assert_eq!(fs.read("a/note.txt").unwrap(), b"hello");
    for path in ["/etc/passwd", "../secret", "a/../../secret", ""] {
        assert!(fs.read(path).is_err());
    }
    assert!(fs.write("a/note.txt", &"x".repeat(65537)).is_err());
    assert_eq!(fs.read("a/note.txt").unwrap(), b"hello");
}
#[cfg(unix)]
#[test]
fn symlinks_are_rejected_without_touching_targets() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "protected").unwrap();
    symlink(outside.path(), root.path().join("escape")).unwrap();
    symlink(outside.path().join("secret"), root.path().join("final")).unwrap();
    let fs = Workspace::open(root.path()).unwrap();
    assert!(fs.read("escape/secret").is_err());
    assert!(fs.write("escape/secret", "bad").is_err());
    assert!(fs.write("final", "bad").is_err());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("secret")).unwrap(),
        "protected"
    );
}

#[test]
fn binary_transfer_preserves_bytes_without_weakening_text_validation() {
    let root = tempfile::tempdir().unwrap();
    let fs = Workspace::open(root.path()).unwrap();
    let bytes = [0, 255, 128, 1];
    fs.write_binary("binary.dat", &bytes).unwrap();
    assert_eq!(fs.read_binary("binary.dat").unwrap(), bytes);
    assert!(fs.read("binary.dat").is_err());
    assert!(fs.write_binary("binary.dat", &vec![0; 65537]).is_err());
}
