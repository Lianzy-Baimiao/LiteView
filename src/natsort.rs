//! 文件名自然排序（`img2` 在 `img10` 前，不分大小写）。
//! Windows 上用资源管理器同款的 `StrCmpLogicalW`，顺序与资源管理器完全一致（中文按拼音）；
//! 其它系统用自己的实现：连续数字按数值比较，其余字符不分大小写比较。

use std::cmp::Ordering;
use std::path::PathBuf;

/// 按文件名自然排序
pub fn sort_paths(paths: &mut [PathBuf]) {
    #[cfg(windows)]
    {
        crate::platform::windows::sort_logical(paths);
    }
    #[cfg(not(windows))]
    {
        paths.sort_by(|a, b| compare(&file_name(a), &file_name(b)));
    }
}

pub fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// 自然排序比较（非 Windows 平台使用，也用于测试）
pub fn compare(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let (m, n) = (take_digits(&mut x), take_digits(&mut y));
                let (mt, nt) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = mt.len().cmp(&nt.len()).then_with(|| mt.cmp(nt));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                let ord = lower(c).cmp(&lower(d));
                if ord != Ordering::Equal {
                    return ord;
                }
                x.next();
                y.next();
            }
        }
    }
}

fn take_digits(it: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut s = String::new();
    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
        s.push(c);
        it.next();
    }
    s
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = ["img10.png", "img2.png", "IMG1.png", "a.png", "img02b.png"];
        v.sort_by(|a, b| compare(a, b));
        assert_eq!(v, ["a.png", "IMG1.png", "img2.png", "img02b.png", "img10.png"]);
        assert_eq!(compare("x9", "x10"), Ordering::Less);
        assert_eq!(compare("photo", "Photo 1"), Ordering::Less);
    }

    #[test]
    fn sort_paths_matches_explorer_basics() {
        let mut v: Vec<PathBuf> = ["b10.jpg", "b2.jpg", "B1.jpg"].iter().map(PathBuf::from).collect();
        sort_paths(&mut v);
        let names: Vec<String> = v.iter().map(|p| file_name(p)).collect();
        assert_eq!(names, ["B1.jpg", "b2.jpg", "b10.jpg"]);
    }
}
