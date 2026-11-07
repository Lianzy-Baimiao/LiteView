//! 资源管理器窗口里显示的顺序（「排序：跟随资源管理器」）：找到正开着某个文件夹的资源管理器窗口，按它显示的先后取出各项的名字。
//! 排序、分组、升降序都由那个窗口决定，这里不重新实现任何一种排序。跨进程问资源管理器，只读，不改它的任何设置。

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;

use windows::Win32::System::Com::{CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IServiceProvider};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Shell::Common::{ITEMIDLIST, STRRET};
use windows::Win32::UI::Shell::{
    IEnumIDList, IFolderView2, IPersistFolder2, IShellBrowser, IShellFolder, IShellWindows, SHBindToObject, SHGDN_FORPARSING, SHGDN_INFOLDER, SHGDNF, SHGetNameFromIDList,
    SID_STopLevelBrowser, SIGDN_FILESYSPATH, SVGIO_ALLVIEW, SVGIO_FLAG_VIEWORDER, ShellWindows, StrRetToStrW,
};
use windows::core::{Interface, PWSTR};

/// 正开着 `dir` 的资源管理器窗口里，各项（文件和文件夹）按显示先后的名字（带扩展名）。好几个窗口（标签页）都开着它时，
/// 优先选焦点在 `prefer` 上的那个：在资源管理器里双击打开的图就是那个窗口的焦点。没有这样的窗口、或读不到时 None。
/// 「此电脑」、库、搜索结果这类不是真实文件夹的窗口不算
pub fn view_order(dir: &Path, prefer: Option<&Path>) -> Option<Vec<OsString>> {
    let _apartment = Apartment::enter()?;
    let windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER) }.ok()?;
    let want = path_key(dir.as_os_str());
    let prefer = prefer.and_then(Path::file_name).map(name_key);
    let mut chosen: Option<(IFolderView2, IShellFolder)> = None;
    for i in 0..unsafe { windows.Count() }.unwrap_or(0) {
        let Some(view) = folder_view(&windows, i) else { continue };
        let Some((path, folder)) = folder_of(&view) else { continue };
        if path_key(&path) != want {
            continue;
        }
        if prefer.is_some() && focused_name(&view, &folder).as_deref().map(name_key) == prefer {
            chosen = Some((view, folder));
            break;
        }
        if chosen.is_none() {
            chosen = Some((view, folder));
        }
    }
    let (view, folder) = chosen?;
    items(&view, &folder)
}

/// 这个线程进入单线程套间，离开时退出。资源管理器的对象都要在它之前释放（局部变量按声明的反序释放，所以它要最先声明）
struct Apartment;

impl Apartment {
    fn enter() -> Option<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok().then_some(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// 资源管理器给的 ITEMIDLIST（CoTaskMemAlloc 分配的），用完释放
struct Pidl(*mut ITEMIDLIST);

impl Drop for Pidl {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0 as *const core::ffi::c_void)) };
    }
}

/// 系统分配的字符串转成 OsString，并释放
fn take_string(s: PWSTR) -> OsString {
    let out = OsString::from_wide(unsafe { s.as_wide() });
    unsafe { CoTaskMemFree(Some(s.0 as *const core::ffi::c_void)) };
    out
}

/// ShellWindows 里第 i 个窗口（Win11 上每个标签页各算一个）当前显示的文件夹视图
fn folder_view(windows: &IShellWindows, i: i32) -> Option<IFolderView2> {
    let mut index = VARIANT::default();
    unsafe {
        (*index.Anonymous.Anonymous).vt = VT_I4;
        (*index.Anonymous.Anonymous).Anonymous.lVal = i;
        let window = windows.Item(&index).ok()?;
        let browser: IShellBrowser = window.cast::<IServiceProvider>().ok()?.QueryService(&SID_STopLevelBrowser).ok()?;
        browser.QueryActiveShellView().ok()?.cast().ok()
    }
}

/// 视图显示的文件夹：文件系统路径，和在本进程里打开的同一个文件夹（取各项的名字用，不再跨进程）
fn folder_of(view: &IFolderView2) -> Option<(OsString, IShellFolder)> {
    unsafe {
        let persist: IPersistFolder2 = view.GetFolder().ok()?;
        let pidl = Pidl(persist.GetCurFolder().ok()?);
        let path = take_string(SHGetNameFromIDList(pidl.0, SIGDN_FILESYSPATH).ok()?);
        let folder: IShellFolder = SHBindToObject(None, pidl.0, None).ok()?;
        Some((path, folder))
    }
}

/// 文件夹里一项的名字（真实文件名，带扩展名；与资源管理器显不显示扩展名无关）
fn child_name(folder: &IShellFolder, child: *const ITEMIDLIST) -> Option<OsString> {
    unsafe {
        let mut name = STRRET::default();
        folder.GetDisplayNameOf(child, SHGDNF(SHGDN_INFOLDER.0 | SHGDN_FORPARSING.0), &mut name).ok()?;
        let mut s = PWSTR::null();
        StrRetToStrW(&mut name, Some(child), &mut s).ok()?;
        Some(take_string(s))
    }
}

/// 视图里有焦点的那一项（双击打开的就是它）
fn focused_name(view: &IFolderView2, folder: &IShellFolder) -> Option<OsString> {
    unsafe {
        let i = view.GetFocusedItem().ok().filter(|&i| i >= 0)?;
        let child = Pidl(view.Item(i).ok()?);
        child_name(folder, child.0)
    }
}

/// 视图里的所有项，按显示的先后（排序、分组都已算在里面）
fn items(view: &IFolderView2, folder: &IShellFolder) -> Option<Vec<OsString>> {
    let list: IEnumIDList = unsafe { view.Items(SVGIO_ALLVIEW | SVGIO_FLAG_VIEWORDER) }.ok()?;
    let mut names = Vec::new();
    loop {
        // 一次取一批，跨进程的次数少
        let mut batch = [std::ptr::null_mut::<ITEMIDLIST>(); 512];
        let mut got = 0u32;
        let hr = unsafe { list.Next(&mut batch, Some(&mut got)) };
        for &child in batch.iter().take(got as usize) {
            let child = Pidl(child);
            if let Some(name) = child_name(folder, child.0) {
                names.push(name);
            }
        }
        if hr.is_err() {
            return (!names.is_empty()).then_some(names);
        }
        // S_FALSE：比要的少，已经到头
        if (got as usize) < batch.len() {
            return Some(names);
        }
    }
}

/// 比较路径用：不分大小写，去掉末尾的分隔符（盘符根目录 `C:\` 与 `C:` 相同）
fn path_key(p: &OsStr) -> String {
    p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase()
}

fn name_key(n: &OsStr) -> String {
    n.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_compare_like_windows() {
        assert_eq!(path_key(OsStr::new(r"C:\Pics\")), path_key(OsStr::new(r"c:\pics")));
        assert_eq!(path_key(OsStr::new(r"D:\")), "d:");
        assert_ne!(path_key(OsStr::new(r"C:\Pics")), path_key(OsStr::new(r"C:\Pics2")));
        assert_eq!(name_key(OsStr::new("IMG_01.JPG")), "img_01.jpg");
    }
}
