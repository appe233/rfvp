use super::*;
use std::io::Write;

#[test]
fn stable_sources_survive_path_replacement_and_have_independent_cursors() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("archive"), b"0123456789").unwrap();
    let root = open_root(dir.path()).unwrap();
    let source = open_source(&root, "archive").unwrap();
    std::fs::rename(dir.path().join("archive"), dir.path().join("old")).unwrap();
    std::fs::write(dir.path().join("archive"), b"XXXXXXXXXX").unwrap();
    let mut a = source.slice(2, 5).unwrap();
    let mut b = source.slice(4, 3).unwrap();
    let mut bytes = [0; 2];
    a.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"23");
    b.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"45");
    a.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"45");
    assert_eq!(a.seek(SeekFrom::End(-2)).unwrap(), 3);
    a.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"56");
    assert_eq!(a.read(&mut bytes).unwrap(), 0);
    assert!(source.read_exact_at(u64::MAX, &mut []).is_err());
    source.read_exact_at(10, &mut []).unwrap();
    assert_eq!(
        source.sha256().unwrap(),
        crate::identity::digest(b"0123456789")
    );
}

#[cfg(unix)]
#[test]
fn confined_open_rejects_escape_symlinks_but_accepts_internal_links() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), b"not an input").unwrap();
    std::fs::write(root.path().join("data"), b"inside").unwrap();
    symlink(outside.path(), root.path().join("escape")).unwrap();
    symlink("data", root.path().join("link")).unwrap();
    let dir = open_root(root.path()).unwrap();
    assert!(open_source(&dir, "escape/secret").is_err());
    let a = open_source(&dir, "data").unwrap();
    let b = open_source(&dir, "link").unwrap();
    assert!(a.same_file(&b));
    for path in ["../secret", "/secret", "a/../data", "a\\data", "a:data"] {
        assert!(open_source(&dir, path).is_err());
    }
}

#[cfg(unix)]
#[test]
fn symlink_replacement_during_open_never_reads_outside_root() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("inside"), b"inside").unwrap();
    std::fs::write(outside.path().join("outside"), b"outside").unwrap();
    symlink("inside", root.path().join("link")).unwrap();
    let dir = open_root(root.path()).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for index in 0..300 {
                let target = if index % 2 == 0 {
                    outside.path().join("outside")
                } else {
                    Path::new("inside").to_path_buf()
                };
                symlink(target, root.path().join("next")).unwrap();
                std::fs::rename(root.path().join("next"), root.path().join("link")).unwrap();
            }
        });
        for _ in 0..300 {
            if let Ok(source) = open_source(&dir, "link") {
                assert_eq!(source.bounded_bytes(16).unwrap(), b"inside");
            }
        }
    });
}

#[test]
fn sparse_sources_exceed_copy_budget_without_whole_file_buffering() {
    let mut file = tempfile::tempfile().unwrap();
    file.set_len(100 * 1024 * 1024).unwrap();
    file.seek(SeekFrom::End(-4)).unwrap();
    file.write_all(b"tail").unwrap();
    let source = SourceFile::new(file).unwrap();
    let mut bytes = [0; 4];
    source.read_exact_at(source.len() - 4, &mut bytes).unwrap();
    assert_eq!(&bytes, b"tail");
    assert!(source.bounded_bytes(crate::abi::MAX_IO).is_err());
}
