//! 安全写入：在同一目录写临时文件 → 刷到磁盘 → 替换原文件（保留原文件的属性与权限）。
//! 任何一步失败，原文件都不变。配置文件与覆盖保存图片都用它。
//!
//! 不在原文件上直接覆盖写入：2023 年的 aCropalypse 漏洞就是覆盖写入后没有截断，
//! 旧图的数据留在了文件尾部。

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// 用 `write` 生成内容并安全地替换 `path`
pub fn atomic_write(path: &Path, write: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> io::Result<()> {
    let tmp = temp_path(path);
    let result = (|| {
        let mut w = BufWriter::new(File::create(&tmp)?);
        write(&mut w)?;
        let f = w.into_inner().map_err(|e| e.into_error())?;
        f.sync_all()?;
        drop(f);
        replace(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".{name}.{}-{n}.tmp", std::process::id()))
}

#[cfg(windows)]
fn replace(tmp: &Path, dst: &Path) -> io::Result<()> {
    if dst.exists() && crate::platform::windows::replace_file(dst, tmp).is_ok() {
        return Ok(());
    }
    std::fs::rename(tmp, dst)
}

#[cfg(not(windows))]
fn replace(tmp: &Path, dst: &Path) -> io::Result<()> {
    if let Ok(meta) = std::fs::metadata(dst) {
        let _ = std::fs::set_permissions(tmp, meta.permissions());
    }
    std::fs::rename(tmp, dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("liteview-test-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn replaces_and_truncates() {
        let d = dir();
        let p = d.join("a.bin");
        std::fs::write(&p, vec![7u8; 4096]).unwrap();
        atomic_write(&p, |w| w.write_all(b"short")).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"short");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "no temp file left");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn failure_keeps_original() {
        let d = dir();
        let p = d.join("b.bin");
        std::fs::write(&p, b"original").unwrap();
        let r = atomic_write(&p, |w| {
            w.write_all(b"partial")?;
            Err(io::Error::other("boom"))
        });
        assert!(r.is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(d);
    }
}
