//! 文件关联（PRD 6.6）。Windows 不允许程序直接改默认应用，所以把本软件注册为能打开这些格式的程序
//! （只写当前用户 HKCU，不需要管理员权限），再打开系统「默认应用」让用户选。用户不点就不写注册表。

pub const APP: &str = "LiteView";

/// 设置页里按组列出的扩展名：常用、更多、专业、RAW
pub const GROUPS: [&[&str]; 4] = [
    &["jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "webp"],
    &["tif", "tiff", "ico", "heic", "heif", "avif", "jxl", "svg"],
    &["tga", "dds", "psd", "exr", "hdr", "qoi"],
    &["cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2"],
];

pub fn default_exts() -> Vec<String> {
    GROUPS[0].iter().chain(GROUPS[1]).map(|s| s.to_string()).collect()
}

pub fn all_exts() -> Vec<String> {
    GROUPS.iter().flat_map(|g| g.iter()).map(|s| s.to_string()).collect()
}

pub fn common_exts() -> Vec<String> {
    GROUPS[0].iter().map(|s| s.to_string()).collect()
}

/// 按界面顺序去重，忽略旧配置中不支持的扩展名。
pub fn selected_exts(exts: &[String]) -> Vec<String> {
    all_exts().into_iter().filter(|ext| exts.contains(ext)).collect()
}

#[cfg(any(windows, test))]
fn is_liteview_prog_id(id: &str) -> bool {
    id.eq_ignore_ascii_case(r"Applications\LiteView.exe") || GROUPS.iter().flat_map(|g| g.iter()).any(|ext| id.eq_ignore_ascii_case(&format!("{APP}.{ext}")))
}

#[cfg(any(windows, test))]
fn launch_default_settings(build: u32, mut launch: impl FnMut(&str) -> Result<(), String>) -> Result<(), String> {
    if build >= 22621 && launch("ms-settings:defaultapps?registeredAppUser=LiteView").is_ok() {
        return Ok(());
    }
    launch("ms-settings:defaultapps")
}

pub fn validate_extensions(exts: &[String]) -> Result<(), String> {
    for ext in exts {
        if !GROUPS.iter().any(|group| group.contains(&ext.as_str())) {
            return Err(format!("Unsupported file association extension: {ext:?}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod validation_tests {
    #[test]
    fn invalid_association_settings_fail_before_registry_access() {
        assert!(super::validate_extensions(&super::all_exts()).is_ok());
        for ext in ["png\\other", "..", "", "PNG", "exe"] {
            assert!(super::validate_extensions(&[ext.into()]).is_err());
        }
    }

    #[test]
    fn selection_preserves_group_order_without_duplicates() {
        assert_eq!(super::selected_exts(&["webp".into(), "jpg".into(), "jpg".into(), "exe".into()]), ["jpg", "webp"]);
        assert!(super::selected_exts(&[]).is_empty());
        assert_eq!(super::common_exts().len(), 8);
    }

    #[test]
    fn default_status_matches_only_our_known_prog_ids() {
        for id in ["LiteView.jpg", "liteview.PNG", r"Applications\LiteView.exe"] {
            assert!(super::is_liteview_prog_id(id));
        }
        for id in ["OtherLiteView.png", "LiteView.png.backup", "LiteView.exe", "", "Photos.jpg"] {
            assert!(!super::is_liteview_prog_id(id));
        }
    }

    #[test]
    fn settings_launch_falls_back_and_reports_failure() {
        let mut urls = Vec::new();
        let result = super::launch_default_settings(26200, |uri| {
            urls.push(uri.to_string());
            if urls.len() == 1 { Err("unavailable".into()) } else { Ok(()) }
        });
        assert!(result.is_ok());
        assert_eq!(urls, ["ms-settings:defaultapps?registeredAppUser=LiteView", "ms-settings:defaultapps"]);
        urls.clear();
        assert!(
            super::launch_default_settings(19045, |uri| {
                urls.push(uri.to_string());
                Ok(())
            })
            .is_ok()
        );
        assert_eq!(urls, ["ms-settings:defaultapps"]);
        assert_eq!(super::launch_default_settings(26200, |_| Err("failed".into())), Err("failed".into()));
        let mut calls = 0;
        assert!(
            super::launch_default_settings(26200, |_| {
                calls += 1;
                Ok(())
            })
            .is_ok()
        );
        assert_eq!(calls, 1);
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::*;
    use windows::Win32::UI::Shell::{ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_PROGID, AssocQueryStringW, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, ShellExecuteW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{PCWSTR, PWSTR, w};

    use super::{APP, GROUPS};

    fn wstr(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn create(path: &str) -> Result<Key, String> {
        let p = wstr(path);
        let mut k = HKEY::default();
        let r = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut k, None) };
        if r == ERROR_SUCCESS { Ok(Key(k)) } else { Err(format!("HKCU\\{path}: {}", r.0)) }
    }

    /// 写字符串值（name 为空 = 默认值）
    fn set(path: &str, name: &str, value: &str) -> Result<(), String> {
        let k = create(path)?;
        let (n, v) = (wstr(name), wstr(value));
        let bytes = unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), v.len() * 2) };
        let name_ptr = if name.is_empty() { PCWSTR::null() } else { PCWSTR(n.as_ptr()) };
        let r = unsafe { RegSetValueExW(k.0, name_ptr, None, REG_SZ, Some(bytes)) };
        if r == ERROR_SUCCESS { Ok(()) } else { Err(format!("HKCU\\{path}\\{name}: {}", r.0)) }
    }

    fn get(root: HKEY, path: &str, name: &str) -> Option<String> {
        let (p, n) = (wstr(path), wstr(name));
        let mut buf = vec![0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(root, PCWSTR(p.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
        (r == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
    }

    fn delete_tree(path: &str) {
        let p = wstr(path);
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()));
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()));
        }
    }

    fn delete_value(path: &str, name: &str) {
        let (p, n) = (wstr(path), wstr(name));
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), PCWSTR(n.as_ptr()));
        }
    }

    /// 没有值也没有子键的键删掉（注册前就不存在的键，取消关联后恢复原样）
    fn delete_if_empty(path: &str) {
        let p = wstr(path);
        let mut k = HKEY::default();
        unsafe {
            if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), None, KEY_READ, &mut k) != ERROR_SUCCESS {
                return;
            }
            let (mut subkeys, mut values) = (0u32, 0u32);
            let r = RegQueryInfoKeyW(k, None, None, None, Some(&mut subkeys), None, None, Some(&mut values), None, None, None, None);
            let _ = RegCloseKey(k);
            if r == ERROR_SUCCESS && subkeys == 0 && values == 0 {
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()));
            }
        }
    }

    fn prog_id(ext: &str) -> String {
        format!("{APP}.{ext}")
    }

    const APP_KEY: &str = r"Software\Classes\Applications\LiteView.exe";
    const VERBS: [(&str, &str); 2] = [("LiteView.edit", "--edit"), ("LiteView.mosaic", "--mosaic")];

    pub fn register(exts: &[String]) -> Result<(), String> {
        super::validate_extensions(exts)?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.to_string_lossy();
        let t = crate::i18n::tr();
        let caps = format!(r"Software\{APP}\Capabilities");
        set(&caps, "ApplicationName", APP)?;
        set(&caps, "ApplicationDescription", t.assoc_desc)?;
        set(r"Software\RegisteredApplications", APP, &caps)?;
        set(&format!(r"{APP_KEY}\shell\open\command"), "", &format!("\"{exe}\" \"%1\""))?;
        set(&format!(r"{APP_KEY}\DefaultIcon"), "", &format!("\"{exe}\",0"))?;
        // 注册是增量操作；保留已有格式，并更新它们的路径以支持移动绿色版或升级。
        let registered = super::all_exts().into_iter().filter(|ext| exts.contains(ext) || exists(&format!(r"Software\Classes\{}", prog_id(ext))));
        for ext in registered {
            let id = prog_id(&ext);
            let base = format!(r"Software\Classes\{id}");
            set(&base, "", &format!("{} ({APP})", ext.to_uppercase()))?;
            set(&format!(r"{base}\DefaultIcon"), "", &format!("\"{exe}\",0"))?;
            set(&format!(r"{base}\shell\open\command"), "", &format!("\"{exe}\" \"%1\""))?;
            set(&format!(r"{base}\shell\edit"), "", t.edit)?;
            set(&format!(r"{base}\shell\edit\command"), "", &format!("\"{exe}\" --edit \"%1\""))?;
            set(&format!(r"Software\Classes\.{ext}\OpenWithProgids"), &id, "")?;
            set(&format!(r"{caps}\FileAssociations"), &format!(".{ext}"), &id)?;
            set(&format!(r"{APP_KEY}\SupportedTypes"), &format!(".{ext}"), "")?;
        }
        // 资源管理器右键（经典菜单；Win11 在「显示更多选项」里）
        for ((verb, flag), label) in VERBS.iter().zip([t.ctx_edit_with, t.ctx_mosaic_with]) {
            let base = format!(r"Software\Classes\SystemFileAssociations\image\shell\{verb}");
            set(&base, "", label)?;
            set(&base, "Icon", &format!("\"{exe}\",0"))?;
            set(&format!(r"{base}\command"), "", &format!("\"{exe}\" {flag} \"%1\""))?;
        }
        notify();
        Ok(())
    }

    fn exists(path: &str) -> bool {
        let p = wstr(path);
        let mut k = HKEY::default();
        let ok = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), None, KEY_READ, &mut k) } == ERROR_SUCCESS;
        if ok {
            drop(Key(k));
        }
        ok
    }

    /// 只清理本软件写过的（没注册过这个扩展名就什么都不动）
    fn remove_ext(ext: &str) {
        let id = prog_id(ext);
        let owp = format!(r"Software\Classes\.{ext}\OpenWithProgids");
        if !exists(&format!(r"Software\Classes\{id}")) && get(HKEY_CURRENT_USER, &owp, &id).is_none() {
            return;
        }
        delete_tree(&format!(r"Software\Classes\{id}"));
        delete_value(&owp, &id);
        delete_if_empty(&owp);
        delete_if_empty(&format!(r"Software\Classes\.{ext}"));
    }

    /// 删除本软件写入的所有注册信息
    pub fn unregister() {
        for ext in GROUPS.iter().flat_map(|g| g.iter()) {
            remove_ext(ext);
        }
        delete_tree(APP_KEY);
        for (verb, _) in VERBS {
            delete_tree(&format!(r"Software\Classes\SystemFileAssociations\image\shell\{verb}"));
        }
        delete_value(r"Software\RegisteredApplications", APP);
        delete_tree(&format!(r"Software\{APP}"));
        notify();
    }

    pub fn is_registered() -> bool {
        get(HKEY_CURRENT_USER, r"Software\RegisteredApplications", APP).is_some()
    }

    /// 查询系统实际解析出的默认处理程序，不把无效或过期的 UserChoice 当成成功。
    pub fn is_default(ext: &str) -> bool {
        let ext = wstr(&format!(".{ext}"));
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let result = unsafe { AssocQueryStringW(ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_PROGID, PCWSTR(ext.as_ptr()), PCWSTR::null(), Some(PWSTR(buf.as_mut_ptr())), &mut len) };
        result.0 == 0 && len > 0 && len as usize <= buf.len() && super::is_liteview_prog_id(&String::from_utf16_lossy(&buf[..len as usize - 1]))
    }

    /// 打开系统「默认应用」；Win11 22H2 起直接到本软件的页面
    pub fn open_default_apps() -> Result<(), String> {
        let build: u32 = get(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuildNumber").and_then(|b| b.parse().ok()).unwrap_or(0);
        super::launch_default_settings(build, |uri| {
            let u = wstr(uri);
            let result = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(u.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
            let code = result.0 as isize;
            if code > 32 { Ok(()) } else { Err(format!("Windows Settings (ShellExecute: {code})")) }
        })
    }

    fn notify() {
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) }
    }
}
