//! 选显卡接口。wgpu 默认同时初始化 Vulkan、DX12、GL，会把几套驱动都载进来；实测（M0）只用 Vulkan 时
//! 空闲内存最低、出第一帧最快。所以按顺序试：Vulkan → DX12（虚拟机里有 WARP 软件渲染兜底）→ GL；
//! macOS 用 Metal。环境变量 WGPU_BACKEND 可以指定。

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew, wgpu};

fn order() -> Vec<wgpu::Backends> {
    if cfg!(target_os = "macos") {
        vec![wgpu::Backends::METAL]
    } else if cfg!(windows) {
        vec![wgpu::Backends::VULKAN, wgpu::Backends::DX12, wgpu::Backends::GL]
    } else {
        vec![wgpu::Backends::VULKAN, wgpu::Backends::GL]
    }
}

/// 第一个有可用显卡的接口
fn pick() -> wgpu::Backends {
    if let Some(b) = wgpu::Backends::from_env() {
        return b;
    }
    for b in order() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: b, ..wgpu::InstanceDescriptor::new_without_display_handle_from_env() });
        let adapters = pollster::block_on(instance.enumerate_adapters(b));
        if !adapters.is_empty() {
            return b;
        }
    }
    wgpu::Backends::all()
}

pub fn configuration() -> WgpuConfiguration {
    let backends = pick();
    log::info!("显卡接口：{backends:?}");
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = backends;
    setup.power_preference = wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::LowPower);
    WgpuConfiguration { wgpu_setup: WgpuSetup::CreateNew(setup), ..Default::default() }
}
