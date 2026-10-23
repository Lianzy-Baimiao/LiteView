//! 选显卡接口。
//!
//! Windows：自己用 Direct3D 11 画（`app::host`、`platform::d3d11`）。D3D11 几乎所有显卡都支持（不要求 DX12），
//! 占内存少：同样一个空窗口，wgpu + Vulkan 约 112 MB、wgpu + DX12 约 96 MB，D3D11 约 20 MB。
//! 硬件创建失败、或这台机器上用它启动时崩溃 / 卡住过，就用 WARP（Windows 自带的软件渲染，没有显卡也能用）。
//!
//! 其它系统：eframe + wgpu。Linux 按 Vulkan → GL；macOS 用 Metal。环境变量 WGPU_BACKEND 可以指定。
//!
//! 「能枚举到显卡」不等于能用：窗口表面不支持、建设备失败、交换链失败、驱动崩溃或卡住都可能发生在之后。
//! 本进程里的失败换下一个接口重试；崩溃 / 卡住由 `startup` 记下来，下次启动跳过。

/// 按优先顺序（`startup.json` 里记的名字）
pub fn preferred() -> &'static [&'static str] {
    if cfg!(windows) {
        &["d3d11", "warp"]
    } else if cfg!(target_os = "macos") {
        &["metal"]
    } else {
        &["vulkan", "gl"]
    }
}

pub fn display_name(name: &str) -> &str {
    match name {
        "d3d11" => "Direct3D 11",
        "warp" => "Direct3D 11 (WARP)",
        "vulkan" => "Vulkan",
        "dx12" => "DirectX 12",
        "gl" => "OpenGL",
        "metal" => "Metal",
        other => other,
    }
}

/// 开发、排查用：环境变量指定的接口（指定了就只用它，不自动换）。
/// Windows 是 `LITEVIEW_RENDERER=d3d11|warp`，其它系统是 `WGPU_BACKEND`
#[cfg(windows)]
pub fn forced() -> Option<&'static str> {
    let v = std::env::var("LITEVIEW_RENDERER").ok()?;
    preferred().iter().copied().find(|n| n.eq_ignore_ascii_case(v.trim()))
}

#[cfg(not(windows))]
pub use wgpu_backend::{configuration, forced, has_adapter};

#[cfg(not(windows))]
mod wgpu_backend {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew, wgpu};

    fn backends(name: &str) -> Option<wgpu::Backends> {
        Some(match name {
            "vulkan" => wgpu::Backends::VULKAN,
            "gl" => wgpu::Backends::GL,
            "metal" => wgpu::Backends::METAL,
            _ => return None,
        })
    }

    pub fn forced() -> Option<wgpu::Backends> {
        wgpu::Backends::from_env()
    }

    /// 这个接口有没有显卡（只初始化这一个接口）
    pub fn has_adapter(name: &str) -> bool {
        let Some(b) = backends(name) else { return false };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: b, ..wgpu::InstanceDescriptor::new_without_display_handle_from_env() });
        !pollster::block_on(instance.enumerate_adapters(b)).is_empty()
    }

    /// 只用这个接口的配置；`name` 不认识时（WGPU_BACKEND 指定）用环境变量里的
    pub fn configuration(name: &str) -> WgpuConfiguration {
        let backends = backends(name).or_else(forced).unwrap_or(wgpu::Backends::all());
        log::info!("显卡接口：{backends:?}");
        let mut setup = WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = backends;
        setup.power_preference = wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::LowPower);
        // 默认按性能一次申请 64 ~ 256 MB 的显存块；看图用不着，按需申请小块（大纹理本来就单独分配）
        let device = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| wgpu::DeviceDescriptor { memory_hints: wgpu::MemoryHints::MemoryUsage, ..device(adapter) });
        WgpuConfiguration { wgpu_setup: WgpuSetup::CreateNew(setup), ..Default::default() }
    }
}
