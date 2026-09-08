use std::fs::{self, File, Metadata};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub(super) fn read(path: &Path, maximum: u64) -> Result<Vec<u8>, ()> {
    if !path.is_absolute() || fs::canonicalize(path).map_err(|_| ())? != path {
        return Err(());
    }
    let uid = fs::metadata("/proc/self").map_err(|_| ())?.uid();
    let before = fs::symlink_metadata(path).map_err(|_| ())?;
    if !before.is_file() || before.uid() != uid || before.mode() & 0o077 != 0 {
        return Err(());
    }
    let mut file = File::open(path).map_err(|_| ())?;
    let opened = file.metadata().map_err(|_| ())?;
    if !same(&before, &opened) {
        return Err(());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(maximum.checked_add(1).ok_or(())?)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    let after = file.metadata().map_err(|_| ())?;
    let named = fs::symlink_metadata(path).map_err(|_| ())?;
    if bytes.len() as u64 > maximum
        || !same(&opened, &after)
        || !same(&after, &named)
        || fs::canonicalize(path).map_err(|_| ())? != path
    {
        return Err(());
    }
    Ok(bytes)
}

fn same(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.uid() == b.uid()
        && a.gid() == b.gid()
        && a.mode() == b.mode()
        && a.nlink() == b.nlink()
        && a.size() == b.size()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
