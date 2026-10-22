//! 选显卡接口。wgpu 默认同时初始化 Vulkan、DX12、GL，会把几套驱动都载进来；实测（M0）只用 Vulkan 时
//! 空闲内存最低、出第一帧最快。所以按顺序试：Vulkan → DX12（虚拟机里有 WARP 软件渲染兜底）→ GL；
//! macOS 用 Metal。环境变量 WGPU_BACKEND 可以指定（指定了就只用它）。
//!
//! 「能枚举到显卡」不等于能用：窗口表面不支持、建设备失败、交换链失败、驱动崩溃或卡住都可能发生在之后。
//! 本进程里的失败由 `app::run` 换下一个接口重试；崩溃 / 卡住由 `startup` 记下来，下次启动跳过。

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew, wgpu};

/// 按优先顺序（`startup.json` 里记的名字）
pub fn preferred() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["metal"]
    } else if cfg!(windows) {
        &["vulkan", "dx12", "gl"]
    } else {
        &["vulkan", "gl"]
    }
}

fn backends(name: &str) -> Option<wgpu::Backends> {
    Some(match name {
        "vulkan" => wgpu::Backends::VULKAN,
        "dx12" => wgpu::Backends::DX12,
        "gl" => wgpu::Backends::GL,
        "metal" => wgpu::Backends::METAL,
        _ => return None,
    })
}

pub fn display_name(name: &str) -> &str {
    match name {
        "vulkan" => "Vulkan",
        "dx12" => "DirectX 12",
        "gl" => "OpenGL",
        "metal" => "Metal",
        other => other,
    }
}

/// 环境变量 WGPU_BACKEND 指定的接口（开发、排查用；不自动换）
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
    WgpuConfiguration { wgpu_setup: WgpuSetup::CreateNew(setup), ..Default::default() }
}
