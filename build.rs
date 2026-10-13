//! 嵌入 Windows 资源：程序图标、清单（每显示器 DPI v2、长路径、通用控件 v6）、版本信息。
//! 资源脚本按 Cargo.toml 的版本号生成到 OUT_DIR（纯 ASCII 路径，避免中文路径的编码问题）。
//! GNU 工具链用 MinGW 的 windres（.cargo/config.toml 的 LITEVIEW_MINGW_BIN，否则从 PATH 找）；MSVC 用 rc.exe。

use std::path::PathBuf;
use std::process::Command;

fn main() {
    for f in ["build.rs", "assets/app.ico", "assets/app.manifest"] {
        println!("cargo:rerun-if-changed={f}");
    }
    println!("cargo:rerun-if-env-changed=LITEVIEW_MINGW_BIN");
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    for f in ["app.ico", "app.manifest"] {
        if std::fs::copy(PathBuf::from("assets").join(f), out.join(f)).is_err() {
            println!("cargo:warning=缺少 assets/{f}（cargo run --example icon 生成图标），程序将不带资源");
            return;
        }
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut n: Vec<u16> = version.split(['.', '-']).filter_map(|s| s.parse().ok()).collect();
    n.resize(4, 0);
    let num = format!("{},{},{},{}", n[0], n[1], n[2], n[3]);
    let rc = format!(
        "1 ICON \"app.ico\"\n1 24 \"app.manifest\"\n1 VERSIONINFO\nFILEVERSION {num}\nPRODUCTVERSION {num}\nFILEOS 0x40004\nFILETYPE 0x1\n\
         BEGIN\n BLOCK \"StringFileInfo\"\n BEGIN\n  BLOCK \"040904B0\"\n  BEGIN\n\
         VALUE \"CompanyName\", \"Lianzy-Baimiao\"\nVALUE \"FileDescription\", \"LiteView\"\nVALUE \"FileVersion\", \"{version}\"\n\
         VALUE \"InternalName\", \"LiteView\"\nVALUE \"LegalCopyright\", \"MIT License\"\nVALUE \"OriginalFilename\", \"LiteView.exe\"\n\
         VALUE \"ProductName\", \"LiteView\"\nVALUE \"ProductVersion\", \"{version}\"\n  END\n END\n\
         BLOCK \"VarFileInfo\"\n BEGIN\n  VALUE \"Translation\", 0x409, 1200\n END\nEND\n"
    );
    let rc_path = out.join("app.rc");
    std::fs::write(&rc_path, rc).expect("write rc");
    if target.contains("gnu") {
        let obj = out.join("app_res.o");
        let bin = std::env::var("LITEVIEW_MINGW_BIN").ok();
        let windres = bin.as_ref().map_or("windres".to_string(), |b| format!("{b}/windres.exe"));
        let mut cmd = Command::new(&windres);
        cmd.current_dir(&out).args(["app.rc", "-O", "coff", "-o"]).arg(&obj);
        if let Some(b) = &bin {
            let path = std::env::var("PATH").unwrap_or_default();
            cmd.env("PATH", format!("{b};{path}"));
        }
        match cmd.status() {
            Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", obj.display()),
            _ => println!("cargo:warning=windres 不可用，程序将不带图标与清单"),
        }
    } else {
        let _ = embed_resource::compile_for(&rc_path, ["LiteView"], embed_resource::NONE).manifest_optional();
    }
}
