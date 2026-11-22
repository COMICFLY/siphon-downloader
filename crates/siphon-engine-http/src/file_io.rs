use siphon_core::Result;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Create or open `.siphon.part`, pre-allocating size when known and larger than current length.
pub fn prepare_part_file(part_path: &Path, total_size: Option<u64>) -> Result<()> {
    if let Some(parent) = part_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(part_path)?;

    if let Some(size) = total_size {
        let meta = file.metadata()?;
        if meta.len() < size {
            file.set_len(size)?;
        }
    }
    Ok(())
}

/// Open an independent OS file handle for lock-free positioned writes from a worker.
pub fn open_worker_file(part_path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).open(part_path)
}

/// Write an entire buffer at an exact byte offset without mutating a shared file cursor.
#[cfg(windows)]
pub fn write_all_at(file: &File, mut offset: u64, mut buf: &[u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let written = file.seek_write(buf, offset)?;
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "seek_write returned 0 bytes",
            ));
        }
        buf = &buf[written..];
        offset = offset.saturating_add(written as u64);
    }
    Ok(())
}

#[cfg(unix)]
pub fn write_all_at(file: &File, mut offset: u64, mut buf: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    while !buf.is_empty() {
        let written = file.write_at(buf, offset)?;
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "write_at returned 0 bytes",
            ));
        }
        buf = &buf[written..];
        offset = offset.saturating_add(written as u64);
    }
    Ok(())
}

/// Atomically rename `.siphon.part` to the final target filename.
pub fn finalize_part_file(part_path: &Path, target_path: &Path) -> Result<()> {
    if target_path.exists() {
        let _ = std::fs::remove_file(target_path);
    }
    std::fs::rename(part_path, target_path)?;
    Ok(())
}
