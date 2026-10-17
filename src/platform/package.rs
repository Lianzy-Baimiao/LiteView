//! Package identity is detected at runtime, so one binary also works outside MSIX.

#[derive(Debug)]
pub struct Identity {
    pub family: String,
    pub application_id: Option<String>,
}

pub fn identity() -> Option<&'static Identity> {
    #[cfg(windows)]
    {
        static IDENTITY: std::sync::OnceLock<Option<Identity>> = std::sync::OnceLock::new();
        IDENTITY.get_or_init(detect).as_ref()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn is_packaged() -> bool {
    identity().is_some()
}

#[cfg(windows)]
fn detect() -> Option<Identity> {
    use windows::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, ERROR_SUCCESS};
    use windows::Win32::Storage::Packaging::Appx::{GetCurrentApplicationUserModelId, GetCurrentPackageFamilyName};
    use windows::core::PWSTR;

    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let result = unsafe { GetCurrentPackageFamilyName(&mut len, Some(PWSTR(buf.as_mut_ptr()))) };
    if result == APPMODEL_ERROR_NO_PACKAGE {
        return None;
    }
    if result != ERROR_SUCCESS || len < 2 || len as usize > buf.len() {
        // An unexpected identity error must not enable classic registration/deletion.
        log::warn!("Cannot resolve package identity: {}", result.0);
        return Some(Identity { family: "unresolved-package".into(), application_id: None });
    }
    let family = String::from_utf16_lossy(&buf[..len as usize - 1]);
    len = buf.len() as u32;
    let result = unsafe { GetCurrentApplicationUserModelId(&mut len, Some(PWSTR(buf.as_mut_ptr()))) };
    let application_id = (result == ERROR_SUCCESS && len > 1 && len as usize <= buf.len()).then(|| String::from_utf16_lossy(&buf[..len as usize - 1]));
    Some(Identity { family, application_id })
}

#[cfg(windows)]
pub fn local_data_dir() -> Option<std::path::PathBuf> {
    use windows::Storage::ApplicationData;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};

    identity()?;
    static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        // Resolve WinRT data on its own apartment, without changing the UI thread's COM mode.
        std::thread::Builder::new()
            .name("package-data".into())
            .spawn(|| {
                unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.ok()?;
                let path = ApplicationData::Current().and_then(|data| data.LocalFolder()).and_then(|folder| folder.Path());
                unsafe { RoUninitialize() };
                match path {
                    Ok(path) => Some(std::path::PathBuf::from(path.to_os_string())),
                    Err(error) => {
                        log::warn!("Cannot resolve package LocalFolder: {error}");
                        None
                    }
                }
            })
            .ok()?
            .join()
            .ok()
            .flatten()
    })
    .clone()
}

#[cfg(any(windows, test))]
pub(crate) fn instance_name(user: &str, family: Option<&str>) -> String {
    match family {
        Some(family) => format!("LiteView-{user}-{family}"),
        None => format!("LiteView-{user}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn packaged_and_unpacked_instances_are_separate() {
        assert_eq!(super::instance_name("alice", None), "LiteView-alice");
        assert_ne!(super::instance_name("alice", None), super::instance_name("alice", Some("ice.LiteView_test")));
        assert_ne!(super::instance_name("alice", Some("ice.LiteView_test")), super::instance_name("bob", Some("ice.LiteView_test")));
    }
}
