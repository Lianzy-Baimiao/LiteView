//! 真机检查「排序：跟随资源管理器」：在临时文件夹里放几张图（文件名、大小、修改时间、类型各不相同），用最小化、不抢焦点的
//! 资源管理器窗口打开它，依次把那个窗口设成「修改时间降序」「大小升序」「按类型分组」，核对 LiteView 读到的顺序与按文件信息
//! 算出来的一样（直接读，和经过浏览列表 `Folder` 两条路）；再在 5000 个文件的文件夹上计时。
//! 最后只关掉自己打开的窗口（标签页），不碰本来就开着的窗口；临时文件夹删掉。
//! `cargo run --release --example explorer_order`。只在 Windows 上有用。
//!
//! `explorer_order hold <文件夹>`：给整个程序的检查用。最小化打开这个文件夹，输出 READY，然后从标准输入读命令：
//! `sort modified-desc` / `sort size` / `sort name` / `group type`（改那个窗口的排序，输出 OK）、`quit`（关窗口，输出 CLOSED）。

#[cfg(windows)]
fn main() {
    win::main();
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
mod win {
    use std::path::Path;
    use std::time::{Duration, Instant, SystemTime};

    use liteview::settings::SortKey;
    use liteview::viewer::folder::{Folder, Order};
    use windows::Win32::Foundation::{HWND, LPARAM, PROPERTYKEY, WPARAM};
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::System::Com::{CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IServiceProvider};
    use windows::Win32::System::Variant::{VARIANT, VT_I4};
    use windows::Win32::UI::Shell::{
        IFolderView2, IPersistFolder2, IShellBrowser, IShellWindows, IWebBrowser2, SHELLEXECUTEINFOW, SHFILEINFOW, SHGFI_TYPENAME, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
        SHGetNameFromIDList, SID_STopLevelBrowser, SIGDN_FILESYSPATH, SORT_ASCENDING, SORT_DESCENDING, SORTCOLUMN, SVGIO_ALLVIEW, ShellExecuteExW, ShellWindows,
    };
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWMINNOACTIVE, WM_CLOSE};
    use windows::core::{GUID, HSTRING, Interface, PCWSTR};

    /// FMTID_Storage 下的几个属性：名称 10、大小 12、修改时间 14、类型 4
    const FMTID_STORAGE: GUID = GUID::from_u128(0xb725f130_47ef_101a_a5f1_02608c9eebac);
    fn key(pid: u32) -> PROPERTYKEY {
        PROPERTYKEY { fmtid: FMTID_STORAGE, pid }
    }
    const NAME: u32 = 10;
    const SIZE: u32 = 12;
    const MODIFIED: u32 = 14;
    const TYPE: u32 = 4;

    /// 示例文件：名字、大小、比现在早几小时
    type Sample = (&'static str, u64, u64);
    const SAMPLES: &[Sample] = &[("a 2.png", 5000, 1), ("a 10.png", 1000, 5), ("b.jpg", 9000, 3), ("c.gif", 3000, 2), ("d.webp", 7000, 6), ("e.bmp", 2000, 4)];

    struct Win {
        view: IFolderView2,
        browser: IWebBrowser2,
        top: isize,
    }

    fn variant(i: i32) -> VARIANT {
        let mut v = VARIANT::default();
        unsafe {
            (*v.Anonymous.Anonymous).vt = VT_I4;
            (*v.Anonymous.Anonymous).Anonymous.lVal = i;
        }
        v
    }

    /// 所有资源管理器窗口（标签页）：显示的路径和所在顶层窗口
    fn shell_windows() -> Vec<(String, Win)> {
        let mut out = Vec::new();
        unsafe {
            let Ok(windows) = CoCreateInstance::<_, IShellWindows>(&ShellWindows, None, CLSCTX_LOCAL_SERVER) else { return out };
            for i in 0..windows.Count().unwrap_or(0) {
                let found = (|| -> Option<(String, Win)> {
                    let disp = windows.Item(&variant(i)).ok()?;
                    let browser_svc: IShellBrowser = disp.cast::<IServiceProvider>().ok()?.QueryService(&SID_STopLevelBrowser).ok()?;
                    let view: IFolderView2 = browser_svc.QueryActiveShellView().ok()?.cast().ok()?;
                    let pidl = view.GetFolder::<IPersistFolder2>().ok()?.GetCurFolder().ok()?;
                    let path = SHGetNameFromIDList(pidl, SIGDN_FILESYSPATH).ok().map(|p| {
                        let s = p.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(p.0 as *const _));
                        s
                    });
                    CoTaskMemFree(Some(pidl as *const _));
                    let browser: IWebBrowser2 = disp.cast().ok()?;
                    let top = browser.HWND().map(|h| h.0).unwrap_or(0);
                    Some((path.unwrap_or_default(), Win { view, browser, top }))
                })();
                out.extend(found);
            }
        }
        out
    }

    fn find(dir: &Path) -> Option<Win> {
        let want = dir.to_string_lossy().to_lowercase();
        shell_windows().into_iter().find(|(p, _)| p.to_lowercase() == want).map(|(_, w)| w)
    }

    /// 最小化、不激活地打开文件夹，等它在 ShellWindows 里出现并列完 `count` 项
    fn open(dir: &Path, count: i32) -> Win {
        let file = HSTRING::from(dir.as_os_str());
        let mut info = SHELLEXECUTEINFOW { cbSize: size_of::<SHELLEXECUTEINFOW>() as u32, lpFile: PCWSTR(file.as_ptr()), nShow: SW_SHOWMINNOACTIVE.0, ..Default::default() };
        unsafe { ShellExecuteExW(&mut info) }.expect("open folder");
        let start = Instant::now();
        loop {
            if let Some(w) = find(dir)
                && unsafe { w.view.ItemCount(SVGIO_ALLVIEW) }.unwrap_or(0) >= count
            {
                return w;
            }
            assert!(start.elapsed() < Duration::from_secs(20), "Explorer window for {} did not appear", dir.display());
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// 只关自己新开的顶层窗口：开始前没有它，而且里面只有这一个标签页。如果资源管理器把文件夹作为标签页加进了本来就开着的
    /// 窗口，不去关（关窗口会连别的标签页一起关），返回 false，留给人手动关
    fn close(w: Win, existing: &[isize]) -> bool {
        let tabs = shell_windows().iter().filter(|(_, o)| o.top == w.top).count();
        if w.top == 0 || existing.contains(&w.top) || tabs != 1 {
            return false;
        }
        unsafe {
            let _ = PostMessageW(Some(HWND(w.top as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        drop(w.browser);
        true
    }

    fn set_view(view: &IFolderView2, sort: u32, descending: bool, group: Option<u32>) {
        let col = SORTCOLUMN { propkey: key(sort), direction: if descending { SORT_DESCENDING } else { SORT_ASCENDING } };
        unsafe {
            let none = PROPERTYKEY::default();
            view.SetGroupBy(group.map(key).as_ref().unwrap_or(&none), true).expect("SetGroupBy");
            view.SetSortColumns(&[col]).expect("SetSortColumns");
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    fn type_name(ext: &str) -> String {
        let mut info = SHFILEINFOW::default();
        let name = HSTRING::from(format!("x.{ext}"));
        unsafe { SHGetFileInfoW(PCWSTR(name.as_ptr()), FILE_ATTRIBUTE_NORMAL, Some(&mut info), size_of::<SHFILEINFOW>() as u32, SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES) };
        String::from_utf16_lossy(&info.szTypeName).trim_end_matches('\0').to_owned()
    }

    /// 示例文件按 cmp 排好后的名字（期望的顺序）
    fn by(cmp: impl Fn(&Sample, &Sample) -> std::cmp::Ordering) -> Vec<&'static str> {
        let mut v: Vec<&Sample> = SAMPLES.iter().collect();
        v.sort_by(|a, b| cmp(a, b));
        v.into_iter().map(|s| s.0).collect()
    }

    fn images(names: impl IntoIterator<Item = String>) -> Vec<String> {
        names.into_iter().filter(|n| liteview::viewer::folder::is_image(Path::new(n))).collect()
    }

    /// 经过浏览列表：打开第一张，等后台扫描（含问资源管理器）完成
    fn via_folder(dir: &Path, first: &str) -> (Vec<String>, f64) {
        let start = Instant::now();
        let mut f = Folder::open(&dir.join(first), Order { key: SortKey::Name, desc: false, explorer: true }, None);
        while f.scanning() {
            f.poll();
            std::thread::sleep(Duration::from_millis(2));
        }
        (f.files.iter().map(|p| liteview::natsort::file_name(p)).collect(), start.elapsed().as_secs_f64() * 1000.0)
    }

    fn check(label: &str, dir: &Path, expected: &[&str], failures: &mut u32) {
        let t = Instant::now();
        let direct = images(liteview::platform::explorer::view_order(dir, None).unwrap_or_default().into_iter().map(|n| n.to_string_lossy().into_owned()));
        let direct_ms = t.elapsed().as_secs_f64() * 1000.0;
        let (listed, folder_ms) = via_folder(dir, expected[0]);
        let ok = direct == expected && listed == expected;
        if !ok {
            *failures += 1;
        }
        println!("{} {label}: 直接读 {direct_ms:.1} ms，浏览列表 {folder_ms:.1} ms", if ok { "PASS" } else { "FAIL" });
        if !ok {
            println!("    期望  {expected:?}\n    直接读 {direct:?}\n    列表  {listed:?}");
        }
    }

    /// 开着一个资源管理器窗口，按标准输入的命令改它的排序
    fn hold(dir: &Path) {
        use std::io::BufRead;
        let existing: Vec<isize> = shell_windows().iter().map(|(_, w)| w.top).collect();
        let count = std::fs::read_dir(dir).map(|d| d.count() as i32).unwrap_or(0);
        let w = open(dir, count);
        println!("READY");
        for line in std::io::stdin().lock().lines() {
            let line = line.unwrap_or_default();
            match line.trim() {
                "sort modified-desc" => set_view(&w.view, MODIFIED, true, None),
                "sort size" => set_view(&w.view, SIZE, false, None),
                "sort name" => set_view(&w.view, NAME, false, None),
                "group type" => set_view(&w.view, NAME, false, Some(TYPE)),
                "quit" => break,
                other => {
                    println!("? {other}");
                    continue;
                }
            }
            println!("OK");
        }
        let closed = close(w, &existing);
        println!("{}", if closed { "CLOSED" } else { "LEFT-OPEN" });
    }

    pub fn main() {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().expect("COM");
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("hold") {
            hold(Path::new(args.get(2).expect("folder")));
            return;
        }
        let before = shell_windows();
        let existing: Vec<isize> = before.iter().map(|(_, w)| w.top).collect();
        let before: Vec<String> = before.into_iter().map(|(p, _)| p).collect();
        println!("开始前开着的资源管理器窗口：{before:?}");
        let root = std::env::temp_dir().join(format!("liteview-explorer-order-{}", std::process::id()));
        let small = root.join("样例 small");
        std::fs::create_dir_all(small.join("sub")).unwrap();
        let now = SystemTime::now();
        for (name, size, hours) in SAMPLES {
            let f = std::fs::File::create(small.join(name)).unwrap();
            f.set_len(*size).unwrap();
            f.set_modified(now - Duration::from_secs(hours * 3600)).unwrap();
        }
        std::fs::write(small.join("notes.txt"), b"not an image").unwrap();

        let mut failures = 0;
        let w = open(&small, SAMPLES.len() as i32 + 2);
        println!("窗口：{}", if existing.contains(&w.top) { "加在已有窗口里的标签页" } else { "新开的窗口" });

        set_view(&w.view, MODIFIED, true, None);
        check("修改时间降序", &small, &by(|a, b| a.2.cmp(&b.2)), &mut failures);
        set_view(&w.view, SIZE, false, None);
        check("大小升序", &small, &by(|a, b| a.1.cmp(&b.1)), &mut failures);
        // 按类型分组（组按类型名升序），组内按名称：类型名由本机的文件类型注册决定
        set_view(&w.view, NAME, false, Some(TYPE));
        let ext = |s: &str| s.rsplit('.').next().unwrap_or("").to_owned();
        let expected = by(|a, b| {
            type_name(&ext(a.0)).to_lowercase().cmp(&type_name(&ext(b.0)).to_lowercase()).then_with(|| liteview::natsort::compare(a.0, b.0))
        });
        let types: Vec<String> = expected.iter().map(|n| type_name(&ext(n))).collect();
        println!("    类型名：{types:?}");
        check("按类型分组", &small, &expected, &mut failures);
        set_view(&w.view, NAME, false, None);
        check("名称升序", &small, &["a 2.png", "a 10.png", "b.jpg", "c.gif", "d.webp", "e.bmp"], &mut failures);
        if !close(w, &existing) {
            println!("测试文件夹成了已有窗口里的标签页，没有去关它，也不再开第二个：请手动关掉 {}", small.display());
            std::process::exit(if failures == 0 { 0 } else { 1 });
        }

        // 5000 个文件：计时
        let big = root.join("big");
        std::fs::create_dir_all(&big).unwrap();
        for i in 0..5000 {
            std::fs::File::create(big.join(format!("img_{i:05}.png"))).unwrap();
        }
        let w = open(&big, 5000);
        set_view(&w.view, NAME, true, None);
        let t = Instant::now();
        let n = liteview::platform::explorer::view_order(&big, None).map_or(0, |v| v.len());
        let direct_ms = t.elapsed().as_secs_f64() * 1000.0;
        let (listed, folder_ms) = via_folder(&big, "img_00000.png");
        let desc_ok = listed.first().map(String::as_str) == Some("img_04999.png") && listed.len() == 5000;
        if !desc_ok {
            failures += 1;
        }
        println!("{} 5000 个文件（名称降序）：直接读 {n} 项 {direct_ms:.1} ms，浏览列表 {folder_ms:.1} ms", if desc_ok { "PASS" } else { "FAIL" });
        close(w, &existing);

        // 没有窗口开着时：按文件名，且不多等
        std::thread::sleep(Duration::from_millis(800));
        let (listed, folder_ms) = via_folder(&small, "a 2.png");
        let ok = listed == ["a 2.png", "a 10.png", "b.jpg", "c.gif", "d.webp", "e.bmp"];
        if !ok {
            failures += 1;
        }
        println!("{} 没有资源管理器窗口：按文件名，浏览列表 {folder_ms:.1} ms", if ok { "PASS" } else { "FAIL" });

        std::thread::sleep(Duration::from_millis(500));
        let after: Vec<String> = shell_windows().into_iter().map(|(p, _)| p).collect();
        println!("结束时开着的资源管理器窗口：{after:?}");
        let left: Vec<String> = after.iter().filter(|p| p.to_lowercase().starts_with(&root.to_string_lossy().to_lowercase())).cloned().collect();
        let mut kept = before.clone();
        kept.retain(|p| !after.contains(p));
        if !kept.is_empty() {
            println!("FAIL 开始前就开着的窗口不见了：{kept:?}");
            failures += 1;
        }
        println!("{} 自己开的窗口都关了{}", if left.is_empty() { "PASS" } else { "FAIL" }, if left.is_empty() { String::new() } else { format!("：还剩 {left:?}") });
        let _ = std::fs::remove_dir_all(&root);
        println!("{}", if failures == 0 && left.is_empty() { "全部通过" } else { "有失败" });
        if failures > 0 || !left.is_empty() {
            std::process::exit(1);
        }
    }
}
