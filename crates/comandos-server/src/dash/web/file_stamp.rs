use std::{fs, os::unix::fs::MetadataExt, path::Path};

/// Detect both in-place writes and atomic replacement with the same size/mtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileStamp {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl FileStamp {
    pub(super) fn read(path: &Path) -> Option<Self> {
        let m = fs::metadata(path).ok()?;
        m.is_file().then(|| Self {
            device: m.dev(),
            inode: m.ino(),
            length: m.len(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
        })
    }
}
