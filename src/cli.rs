//! 命令行参数：`LiteView <文件>`、`--edit <文件>`、`--mosaic <文件>`、`--register`、`--unregister`

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Default, Debug, PartialEq)]
pub struct Args {
    pub path: Option<PathBuf>,
    /// 打开后直接进入编辑
    pub edit: bool,
    /// 打开后直接进入编辑并选中打码工具
    pub mosaic: bool,
    /// 注册文件关联后退出（给安装程序用）
    pub register: bool,
    /// 删除文件关联后退出
    pub unregister: bool,
    /// Write local runtime diagnostics to the specified path, then exit without opening a window.
    pub diagnostics: bool,
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Args {
    let mut out = Args::default();
    for arg in args {
        match arg.to_str() {
            Some("--edit") => out.edit = true,
            Some("--mosaic") => out.mosaic = true,
            Some("--register") => out.register = true,
            Some("--unregister") => out.unregister = true,
            Some("--diagnostics") => out.diagnostics = true,
            Some(s) if s.starts_with("--") => {}
            _ if out.path.is_none() => {
                let p = PathBuf::from(arg);
                out.path = Some(std::path::absolute(&p).unwrap_or(p));
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: &[&str]) -> Args {
        parse(v.iter().map(OsString::from))
    }

    #[test]
    fn flags_and_path() {
        let a = p(&["--mosaic", "a.png", "b.png", "--unknown"]);
        assert!(a.mosaic && !a.edit);
        assert!(a.path.unwrap().ends_with("a.png"));
        assert_eq!(p(&[]), Args::default());
        assert!(p(&["--register"]).register);
        let report = p(&["--diagnostics", "report with spaces.json"]);
        assert!(report.diagnostics);
        assert!(report.path.unwrap().ends_with("report with spaces.json"));
    }
}
