//! egui 的 Direct3D 11 画法（只给 Windows 用）。与 egui-wgpu 0.36.2 画出来逐像素一致：同样的着色器（含抖动）、
//! 顶点格式、混合方式、剪裁取整与采样器；非 sRGB 的 B8G8R8A8_UNORM 交换链（与 wgpu 优先选的 Bgra8Unorm 相同）。
//! 先试硬件（特性级 11.1 ~ 10.0），不行用 WARP 软件渲染；翻转模型交换链不行时用旧的 DISCARD 模式。

use std::collections::HashMap;

use egui::epaint::{ClippedPrimitive, ImageDelta, ImageData, Primitive, Vertex};
use egui::{Color32, ColorImage, Rect, TextureFilter, TextureId, TextureOptions, TextureWrapMode, TexturesDelta};
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::{Interface, s};

/// 编好的着色器（源码 assets/shaders/egui.hlsl，用 `cargo run --release --example compile_shaders` 重新生成）
const VS: &[u8] = include_bytes!("../../assets/shaders/egui_vs.dxbc");
const PS: &[u8] = include_bytes!("../../assets/shaders/egui_ps.dxbc");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driver {
    Hardware,
    /// Windows 自带的软件渲染
    Warp,
}

/// 显卡设备丢了（驱动更新、被重置）或画不下去了
#[derive(Debug)]
pub struct Lost(pub String);

impl std::fmt::Display for Lost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

struct Texture {
    srv: ID3D11ShaderResourceView,
    tex: ID3D11Texture2D,
    options: TextureOptions,
}

/// 与着色器里的 cbuffer Locals 一致（16 字节）
#[repr(C)]
struct Locals {
    screen_size: [f32; 2],
    dithering: u32,
    predictable: u32,
}

pub struct Painter {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    swap: IDXGISwapChain1,
    rtv: Option<ID3D11RenderTargetView>,
    size: [u32; 2],
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    blend: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    locals: ID3D11Buffer,
    vb: Option<(ID3D11Buffer, usize)>,
    ib: Option<(ID3D11Buffer, usize)>,
    textures: HashMap<TextureId, Texture>,
    samplers: HashMap<TextureOptions, ID3D11SamplerState>,
    /// 显卡名、特性级（日志、「关于」用）
    pub adapter: String,
    pub max_texture_side: usize,
}

/// 纹理一次交给驱动的上限（字节）
const UPLOAD_CHUNK: usize = 4 << 20;

fn fail(what: &str) -> windows::core::Error {
    windows::core::Error::new(windows::Win32::Foundation::E_FAIL, what)
}

impl Painter {
    pub fn new(hwnd: HWND, driver: Driver, size: [u32; 2]) -> windows::core::Result<Self> {
        let device_type = match driver {
            Driver::Hardware => D3D_DRIVER_TYPE_HARDWARE,
            Driver::Warp => D3D_DRIVER_TYPE_WARP,
        };
        let (mut device, mut ctx, mut level) = (None, None, D3D_FEATURE_LEVEL::default());
        let create = |levels: &[D3D_FEATURE_LEVEL], device: &mut Option<ID3D11Device>, ctx: &mut Option<ID3D11DeviceContext>, level: &mut D3D_FEATURE_LEVEL| unsafe {
            D3D11CreateDevice(None, device_type, HMODULE::default(), D3D11_CREATE_DEVICE_FLAG(0), Some(levels), D3D11_SDK_VERSION, Some(device), Some(level), Some(ctx))
        };
        let all = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
        // 没有 11.1 运行时的系统拿到整个列表会返回 E_INVALIDARG：去掉 11.1 再试
        if create(&all, &mut device, &mut ctx, &mut level).is_err() {
            create(&all[1..], &mut device, &mut ctx, &mut level)?;
        }
        let (device, ctx) = (device.ok_or_else(|| fail("no device"))?, ctx.ok_or_else(|| fail("no context"))?);
        let max_texture_side = if level.0 >= D3D_FEATURE_LEVEL_11_0.0 { 16384 } else { 8192 };
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter = unsafe { dxgi.GetAdapter()? };
        let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
        let name = unsafe { adapter.cast::<IDXGIAdapter1>().ok().and_then(|a| a.GetDesc1().ok()) }
            .map(|d| String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_owned())
            .unwrap_or_default();
        let level_name = format!("{}.{}", level.0 >> 12, (level.0 >> 8) & 0xF);
        // 翻转模型（Windows 10 起都支持）；不行时（个别远程桌面 / 旧驱动）用旧的 DISCARD 模式
        let mut desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size[0].max(1),
            Height: size[1].max(1),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            ..Default::default()
        };
        let swap = match unsafe { factory.CreateSwapChainForHwnd(&device, hwnd, &desc, None, None::<&IDXGIOutput>) } {
            Ok(s) => s,
            Err(e) => {
                log::warn!("翻转模型交换链创建失败（{e}），改用 DISCARD 模式");
                desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
                desc.BufferCount = 1;
                unsafe { factory.CreateSwapChainForHwnd(&device, hwnd, &desc, None, None::<&IDXGIOutput>)? }
            }
        };
        // 全屏由程序自己做（无边框），不要 DXGI 的 Alt+Enter 独占全屏
        unsafe {
            let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER);
        }
        let (mut vs, mut ps, mut layout) = (None, None, None);
        unsafe {
            device.CreateVertexShader(VS, None, Some(&mut vs))?;
            device.CreatePixelShader(PS, None, Some(&mut ps))?;
        }
        // 与 egui-wgpu 一样：pos、uv 两个 float2，颜色按 u32 读
        let elements = [
            D3D11_INPUT_ELEMENT_DESC { SemanticName: s!("POSITION"), Format: DXGI_FORMAT_R32G32_FLOAT, AlignedByteOffset: 0, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, ..Default::default() },
            D3D11_INPUT_ELEMENT_DESC { SemanticName: s!("TEXCOORD"), Format: DXGI_FORMAT_R32G32_FLOAT, AlignedByteOffset: 8, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, ..Default::default() },
            D3D11_INPUT_ELEMENT_DESC { SemanticName: s!("COLOR"), Format: DXGI_FORMAT_R32_UINT, AlignedByteOffset: 16, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, ..Default::default() },
        ];
        unsafe { device.CreateInputLayout(&elements, VS, Some(&mut layout))? };
        // 预乘透明度（egui-wgpu：颜色 One / OneMinusSrcAlpha，透明度 OneMinusDstAlpha / One）
        let mut bd = D3D11_BLEND_DESC::default();
        bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_ONE,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_INV_DEST_ALPHA,
            DestBlendAlpha: D3D11_BLEND_ONE,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ScissorEnable: true.into(), ..Default::default() };
        let cd = D3D11_BUFFER_DESC {
            ByteWidth: size_of::<Locals>() as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let (mut blend, mut raster, mut locals) = (None, None, None);
        unsafe {
            device.CreateBlendState(&bd, Some(&mut blend))?;
            device.CreateRasterizerState(&rd, Some(&mut raster))?;
            device.CreateBuffer(&cd, None, Some(&mut locals))?;
        }
        let mut painter = Self {
            device,
            ctx,
            swap,
            rtv: None,
            size: [desc.Width, desc.Height],
            vs: vs.ok_or_else(|| fail("vs"))?,
            ps: ps.ok_or_else(|| fail("ps"))?,
            layout: layout.ok_or_else(|| fail("layout"))?,
            blend: blend.ok_or_else(|| fail("blend"))?,
            raster: raster.ok_or_else(|| fail("raster"))?,
            locals: locals.ok_or_else(|| fail("locals"))?,
            vb: None,
            ib: None,
            textures: HashMap::new(),
            samplers: HashMap::new(),
            adapter: format!("{name}, feature level {level_name}"),
            max_texture_side,
        };
        painter.create_rtv()?;
        Ok(painter)
    }

    fn create_rtv(&mut self) -> windows::core::Result<()> {
        let back: ID3D11Texture2D = unsafe { self.swap.GetBuffer(0)? };
        let mut rtv = None;
        unsafe { self.device.CreateRenderTargetView(&back, None, Some(&mut rtv))? };
        self.rtv = rtv;
        Ok(())
    }

    /// 窗口大小变了（物理像素）。最小化时是 0，不改
    pub fn resize(&mut self, size: [u32; 2]) -> Result<(), Lost> {
        if size[0] == 0 || size[1] == 0 || size == self.size {
            return Ok(());
        }
        // 先放掉对后台缓冲的所有引用
        self.rtv = None;
        unsafe {
            self.ctx.OMSetRenderTargets(None, None::<&ID3D11DepthStencilView>);
            self.ctx.Flush();
            self.swap.ResizeBuffers(0, size[0], size[1], DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)).map_err(lost)?;
        }
        self.size = size;
        self.create_rtv().map_err(lost)
    }

    /// 画一帧并显示。screenshot = 显示前把画面读回来（开发脚本截图用）
    pub fn paint(&mut self, clear: [f32; 4], ppp: f32, prims: &[ClippedPrimitive], delta: &TexturesDelta, screenshot: bool) -> Result<Option<ColorImage>, Lost> {
        for (id, deltas) in &delta.set {
            for d in deltas {
                self.set_texture(*id, d);
            }
        }
        self.draw(clear, ppp, prims).map_err(lost)?;
        let shot = if screenshot { self.capture().map_err(lost)? } else { None };
        let hr = unsafe { self.swap.Present(1, DXGI_PRESENT(0)) };
        if !delta.free.is_empty() {
            for id in &delta.free {
                self.textures.remove(id);
            }
            // D3D11 推迟销毁：还绑着的、或没 Flush 过的资源不会放掉。放了大图后往往就静止不画了，这里解绑并 Flush
            unsafe {
                self.ctx.PSSetShaderResources(0, Some(&[None]));
                self.ctx.Flush();
            }
        }
        if hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET {
            let reason = unsafe { self.device.GetDeviceRemovedReason() }.err().map_or_else(|| format!("{hr:?}"), |e| e.to_string());
            return Err(Lost(format!("Direct3D device removed: {reason}")));
        }
        Ok(shot)
    }

    fn set_texture(&mut self, id: TextureId, d: &ImageDelta) {
        let ImageData::Color(image) = &d.image;
        let [w, h] = image.size;
        if w == 0 || h == 0 {
            return;
        }
        let bytes = color_bytes(&image.pixels);
        if let Some(pos) = d.pos {
            let Some(t) = self.textures.get_mut(&id) else {
                log::warn!("partial update of unknown texture {id:?}");
                return;
            };
            t.options = d.options;
            let tex = t.tex.clone();
            if let Err(e) = self.upload(&tex, [pos[0] as u32, pos[1] as u32], [w as u32, h as u32], bytes) {
                log::warn!("update texture {w}x{h} failed: {e}");
            }
            return;
        }
        match self.create_texture(w as u32, h as u32, bytes) {
            Ok((tex, srv)) => {
                self.textures.insert(id, Texture { srv, tex, options: d.options });
            }
            Err(e) => log::warn!("create texture {w}x{h} failed: {e}"),
        }
    }

    fn create_texture(&self, w: u32, h: u32, bytes: &[u8]) -> windows::core::Result<(ID3D11Texture2D, ID3D11ShaderResourceView)> {
        // 与 egui-wgpu 一样：不是 sRGB 格式（着色器里按 gamma 空间算）
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        let init = D3D11_SUBRESOURCE_DATA { pSysMem: bytes.as_ptr().cast(), SysMemPitch: w * 4, SysMemSlicePitch: 0 };
        let small = bytes.len() <= UPLOAD_CHUNK;
        let (mut tex, mut srv) = (None, None);
        unsafe {
            self.device.CreateTexture2D(&desc, small.then_some(&raw const init), Some(&mut tex))?;
            let tex = tex.ok_or_else(|| fail("texture"))?;
            if !small {
                self.upload(&tex, [0, 0], [w, h], bytes)?;
            }
            self.device.CreateShaderResourceView(&tex, None, Some(&mut srv))?;
            Ok((tex, srv.ok_or_else(|| fail("srv"))?))
        }
    }

    /// 把 size 大小的像素写到纹理的 pos 处。大块经自己的一小块中转纹理分条拷：整块交给驱动
    /// （初始数据或 UpdateSubresource）的话，驱动另开同样大小的中转内存，纹理放掉以后还一直留着
    fn upload(&self, tex: &ID3D11Texture2D, pos: [u32; 2], size: [u32; 2], bytes: &[u8]) -> windows::core::Result<()> {
        let [w, h] = size;
        let pitch = w as usize * 4;
        if bytes.len() <= UPLOAD_CHUNK {
            let b = D3D11_BOX { left: pos[0], top: pos[1], front: 0, right: pos[0] + w, bottom: pos[1] + h, back: 1 };
            unsafe { self.ctx.UpdateSubresource(tex, 0, Some(&b), bytes.as_ptr().cast(), pitch as u32, 0) };
            return Ok(());
        }
        let started = std::time::Instant::now();
        let rows = (UPLOAD_CHUNK / pitch).clamp(1, h as usize);
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: rows as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let mut staging = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut staging))? };
        let staging = staging.ok_or_else(|| fail("staging"))?;
        for y in (0..h as usize).step_by(rows) {
            let n = rows.min(h as usize - y);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe {
                // 上一条还没拷完时 Map 会等它
                self.ctx.Map(&staging, 0, D3D11_MAP_WRITE, 0, Some(&mut mapped))?;
                for r in 0..n {
                    let src = &bytes[(y + r) * pitch..(y + r + 1) * pitch];
                    std::ptr::copy_nonoverlapping(src.as_ptr(), mapped.pData.cast::<u8>().add(r * mapped.RowPitch as usize), pitch);
                }
                self.ctx.Unmap(&staging, 0);
                let b = D3D11_BOX { left: 0, top: 0, front: 0, right: w, bottom: n as u32, back: 1 };
                self.ctx.CopySubresourceRegion(tex, 0, pos[0], pos[1] + y as u32, 0, &staging, 0, Some(&b));
            }
        }
        log::debug!("上传纹理 {w}×{h}：{:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
        Ok(())
    }

    fn sampler(&mut self, o: TextureOptions) -> Option<ID3D11SamplerState> {
        if let Some(s) = self.samplers.get(&o) {
            return Some(s.clone());
        }
        // egui-wgpu 只设放大 / 缩小（mipmap 用默认的最近邻；纹理只有一级，无所谓）
        let filter = match (o.minification, o.magnification) {
            (TextureFilter::Nearest, TextureFilter::Nearest) => D3D11_FILTER_MIN_MAG_MIP_POINT,
            (TextureFilter::Nearest, TextureFilter::Linear) => D3D11_FILTER_MIN_POINT_MAG_LINEAR_MIP_POINT,
            (TextureFilter::Linear, TextureFilter::Nearest) => D3D11_FILTER_MIN_LINEAR_MAG_MIP_POINT,
            (TextureFilter::Linear, TextureFilter::Linear) => D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
        };
        let address = match o.wrap_mode {
            TextureWrapMode::ClampToEdge => D3D11_TEXTURE_ADDRESS_CLAMP,
            TextureWrapMode::Repeat => D3D11_TEXTURE_ADDRESS_WRAP,
            TextureWrapMode::MirroredRepeat => D3D11_TEXTURE_ADDRESS_MIRROR,
        };
        let desc = D3D11_SAMPLER_DESC {
            Filter: filter,
            AddressU: address,
            AddressV: address,
            AddressW: address,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            MinLOD: 0.0,
            MaxLOD: 32.0,
            ..Default::default()
        };
        let mut s = None;
        unsafe { self.device.CreateSamplerState(&desc, Some(&mut s)) }.ok()?;
        let s = s?;
        self.samplers.insert(o, s.clone());
        Some(s)
    }

    fn draw(&mut self, clear: [f32; 4], ppp: f32, prims: &[ClippedPrimitive]) -> windows::core::Result<()> {
        let Some(rtv) = self.rtv.clone() else { return Ok(()) };
        unsafe { self.ctx.ClearRenderTargetView(&rtv, &clear) };
        // 所有网格拼进同一个顶点 / 索引缓冲，每个网格记下起点
        let (mut nv, mut ni) = (0usize, 0usize);
        for p in prims {
            if let Primitive::Mesh(m) = &p.primitive {
                nv += m.vertices.len();
                ni += m.indices.len();
            }
        }
        if ni == 0 {
            return Ok(());
        }
        let vb = grow(&self.device, &mut self.vb, nv * size_of::<Vertex>(), D3D11_BIND_VERTEX_BUFFER)?;
        let ib = grow(&self.device, &mut self.ib, ni * 4, D3D11_BIND_INDEX_BUFFER)?;
        write(&self.ctx, &vb, |dst| {
            let mut o = 0;
            for p in prims {
                if let Primitive::Mesh(m) = &p.primitive {
                    let b = vertex_bytes(&m.vertices);
                    dst[o..o + b.len()].copy_from_slice(b);
                    o += b.len();
                }
            }
        })?;
        write(&self.ctx, &ib, |dst| {
            let mut o = 0;
            for p in prims {
                if let Primitive::Mesh(m) = &p.primitive {
                    for i in &m.indices {
                        dst[o..o + 4].copy_from_slice(&i.to_ne_bytes());
                        o += 4;
                    }
                }
            }
        })?;
        let [w, h] = self.size;
        let locals = Locals { screen_size: [w as f32 / ppp, h as f32 / ppp], dithering: 1, predictable: 0 };
        write(&self.ctx, &self.locals, |dst| {
            let src = unsafe { std::slice::from_raw_parts((&raw const locals).cast::<u8>(), size_of::<Locals>()) };
            dst[..src.len()].copy_from_slice(src);
        })?;
        unsafe {
            self.ctx.OMSetRenderTargets(Some(&[Some(rtv)]), None::<&ID3D11DepthStencilView>);
            self.ctx.OMSetBlendState(&self.blend, Some(&[0.0; 4]), u32::MAX);
            self.ctx.RSSetState(&self.raster);
            self.ctx.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: w as f32, Height: h as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
            self.ctx.IASetInputLayout(&self.layout);
            self.ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            let stride = size_of::<Vertex>() as u32;
            let vbs = [Some(vb)];
            self.ctx.IASetVertexBuffers(0, 1, Some(vbs.as_ptr()), Some(&stride), Some(&0));
            self.ctx.IASetIndexBuffer(&ib, DXGI_FORMAT_R32_UINT, 0);
            // 常量缓冲按阶段绑定：片元着色器也要（抖动开关在里面）
            let locals = [Some(self.locals.clone())];
            self.ctx.VSSetShader(&self.vs, None);
            self.ctx.VSSetConstantBuffers(0, Some(&locals));
            self.ctx.PSSetShader(&self.ps, None);
            self.ctx.PSSetConstantBuffers(0, Some(&locals));
        }
        let (mut v0, mut i0) = (0usize, 0usize);
        for p in prims {
            let Primitive::Mesh(m) = &p.primitive else {
                log::warn!("paint callbacks are not supported");
                continue;
            };
            let (start_v, start_i) = (v0, i0);
            v0 += m.vertices.len();
            i0 += m.indices.len();
            let r = scissor(p.clip_rect, ppp, self.size);
            if r.right <= r.left || r.bottom <= r.top || m.indices.is_empty() {
                continue;
            }
            let Some((srv, options)) = self.textures.get(&m.texture_id).map(|t| (t.srv.clone(), t.options)) else { continue };
            let Some(sampler) = self.sampler(options) else { continue };
            unsafe {
                self.ctx.RSSetScissorRects(Some(&[r]));
                self.ctx.PSSetShaderResources(0, Some(&[Some(srv)]));
                self.ctx.PSSetSamplers(0, Some(&[Some(sampler)]));
                self.ctx.DrawIndexed(m.indices.len() as u32, start_i as u32, start_v as i32);
            }
        }
        Ok(())
    }

    /// 显示前读回当前后台缓冲（BGRA → egui 的预乘 RGBA）
    fn capture(&self) -> windows::core::Result<Option<ColorImage>> {
        let back: ID3D11Texture2D = unsafe { self.swap.GetBuffer(0)? };
        let [w, h] = self.size;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut staging = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut staging))? };
        let staging = staging.ok_or_else(|| fail("staging"))?;
        let mut pixels = Vec::with_capacity(w as usize * h as usize);
        unsafe {
            self.ctx.CopyResource(&staging, &back);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            for y in 0..h as usize {
                let row = std::slice::from_raw_parts((mapped.pData as *const u8).add(y * mapped.RowPitch as usize), w as usize * 4);
                pixels.extend(row.as_chunks::<4>().0.iter().map(|p| Color32::from_rgba_premultiplied(p[2], p[1], p[0], p[3])));
            }
            self.ctx.Unmap(&staging, 0);
        }
        Ok(Some(ColorImage::new([w as usize, h as usize], pixels)))
    }
}

fn lost(e: windows::core::Error) -> Lost {
    Lost(e.to_string())
}

/// 动态缓冲不够大就换一个（多留一半）
fn grow(device: &ID3D11Device, slot: &mut Option<(ID3D11Buffer, usize)>, need: usize, bind: D3D11_BIND_FLAG) -> windows::core::Result<ID3D11Buffer> {
    if let Some((b, cap)) = slot
        && *cap >= need
    {
        return Ok(b.clone());
    }
    let cap = (need + need / 2).max(64 * 1024);
    let desc = D3D11_BUFFER_DESC { ByteWidth: cap as u32, Usage: D3D11_USAGE_DYNAMIC, BindFlags: bind.0 as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32, ..Default::default() };
    let mut b = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut b))? };
    let b = b.ok_or_else(|| fail("buffer"))?;
    *slot = Some((b.clone(), cap));
    Ok(b)
}

/// 整个动态缓冲重写（WRITE_DISCARD）
fn write(ctx: &ID3D11DeviceContext, buf: &ID3D11Buffer, fill: impl FnOnce(&mut [u8])) -> windows::core::Result<()> {
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { buf.GetDesc(&mut desc) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        ctx.Map(buf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
        fill(std::slice::from_raw_parts_mut(mapped.pData.cast::<u8>(), desc.ByteWidth as usize));
        ctx.Unmap(buf, 0);
    }
    Ok(())
}

fn color_bytes(pixels: &[Color32]) -> &[u8] {
    // Color32 是 #[repr(C)] 的 [u8; 4]
    unsafe { std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), std::mem::size_of_val(pixels)) }
}

fn vertex_bytes(v: &[Vertex]) -> &[u8] {
    // Vertex 是 #[repr(C)]：pos [f32; 2]、uv [f32; 2]、color Color32，共 20 字节，没有填充
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

/// 剪裁矩形：与 egui-wgpu 的 ScissorRect 一样先乘缩放比例、各自四舍五入，再限制在画面内
fn scissor(clip: Rect, ppp: f32, size: [u32; 2]) -> RECT {
    let r = |v: f32| (v * ppp).round().max(0.0) as u32;
    let (x0, y0) = (r(clip.min.x).min(size[0]), r(clip.min.y).min(size[1]));
    let (x1, y1) = (r(clip.max.x).clamp(x0, size[0]), r(clip.max.y).clamp(y0, size[1]));
    RECT { left: x0 as i32, top: y0 as i32, right: x1 as i32, bottom: y1 as i32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scissor_rounds_and_clamps_like_egui_wgpu() {
        let r = scissor(Rect::from_min_max(egui::pos2(-5.0, 10.2), egui::pos2(100.3, 1e9)), 1.5, [120, 400]);
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 15, 120, 400));
        let empty = scissor(Rect::from_min_max(egui::pos2(50.0, 50.0), egui::pos2(40.0, 60.0)), 1.0, [100, 100]);
        assert_eq!((empty.left, empty.right), (50, 50));
        let inf = scissor(Rect::EVERYTHING, 1.25, [800, 600]);
        assert_eq!((inf.left, inf.top, inf.right, inf.bottom), (0, 0, 800, 600));
    }

    #[test]
    fn compiled_shaders_match_the_hlsl_source() {
        let src = include_str!("../../assets/shaders/egui.hlsl").replace("\r\n", "\n");
        let hash = src.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
        assert_eq!(format!("{hash:016x}"), include_str!("../../assets/shaders/egui.hash").trim(), "egui.hlsl changed: run `cargo run --release --example compile_shaders`");
    }

    #[test]
    fn compiled_shaders_load_on_warp() {
        let (mut device, mut ctx) = (None, None);
        let levels = [D3D_FEATURE_LEVEL_10_0];
        unsafe { D3D11CreateDevice(None, D3D_DRIVER_TYPE_WARP, HMODULE::default(), D3D11_CREATE_DEVICE_FLAG(0), Some(&levels), D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut ctx)) }.unwrap();
        let device = device.unwrap();
        let (mut vs, mut ps, mut layout) = (None, None, None);
        let elements = [
            D3D11_INPUT_ELEMENT_DESC { SemanticName: s!("POSITION"), Format: DXGI_FORMAT_R32G32_FLOAT, AlignedByteOffset: 0, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, ..Default::default() },
            D3D11_INPUT_ELEMENT_DESC { SemanticName: s!("TEXCOORD"), Format: DXGI_FORMAT_R32G32_FLOAT, AlignedByteOffset: 8, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, ..Default::default() },
            D3D11_INPUT_ELEMENT_DESC { SemanticName: s!("COLOR"), Format: DXGI_FORMAT_R32_UINT, AlignedByteOffset: 16, InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, ..Default::default() },
        ];
        unsafe {
            device.CreateVertexShader(VS, None, Some(&mut vs)).unwrap();
            device.CreatePixelShader(PS, None, Some(&mut ps)).unwrap();
            device.CreateInputLayout(&elements, VS, Some(&mut layout)).unwrap();
        }
        assert!(vs.is_some() && ps.is_some() && layout.is_some());
        assert_eq!(size_of::<Vertex>(), 20);
        assert_eq!(size_of::<Locals>(), 16);
    }

    /// 不显示的测试窗口（交换链要一个）
    fn test_window() -> HWND {
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, WINDOW_EX_STYLE, WS_POPUP};
        unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), windows::core::w!("STATIC"), windows::core::PCWSTR::null(), WS_POPUP, 0, 0, 64, 64, None, None, None, None) }.unwrap()
    }

    /// 把纹理读回来（RGBA 字节）
    fn read_back(p: &Painter, tex: &ID3D11Texture2D) -> Vec<u8> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };
        let (w, h) = (desc.Width as usize, desc.Height as usize);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut staging = None;
        unsafe { p.device.CreateTexture2D(&desc, None, Some(&mut staging)) }.unwrap();
        let staging = staging.unwrap();
        let mut out = Vec::with_capacity(w * h * 4);
        unsafe {
            p.ctx.CopyResource(&staging, tex);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            p.ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap();
            for y in 0..h {
                out.extend_from_slice(std::slice::from_raw_parts((mapped.pData as *const u8).add(y * mapped.RowPitch as usize), w * 4));
            }
            p.ctx.Unmap(&staging, 0);
        }
        out
    }

    /// 超过 UPLOAD_CHUNK 的整张上传和局部更新走分条，读回来逐字节一致
    #[test]
    fn large_uploads_arrive_intact() {
        let hwnd = test_window();
        let mut painter = Painter::new(hwnd, Driver::Warp, [64, 64]).unwrap();
        let (w, h) = (1500, 1000);
        let pattern = |w: usize, h: usize, seed: usize| -> Vec<Color32> {
            (0..w * h).map(|i| Color32::from_rgba_premultiplied(((i + seed) % 251) as u8, (i / w % 253) as u8, (i % w % 241) as u8, 255)).collect()
        };
        let id = TextureId::Managed(1);
        let full = pattern(w, h, 0);
        assert!(w * h * 4 > UPLOAD_CHUNK);
        painter.set_texture(id, &ImageDelta::full(ColorImage::new([w, h], full.clone()), TextureOptions::NEAREST));
        // 右下 1200×900 换一块（也超过 UPLOAD_CHUNK），位置不在原点
        let (px, py, pw, ph) = (300, 100, 1200, 900);
        assert!(pw * ph * 4 > UPLOAD_CHUNK);
        let part = pattern(pw, ph, 7);
        painter.set_texture(id, &ImageDelta::partial([px, py], ColorImage::new([pw, ph], part.clone()), TextureOptions::NEAREST));
        let mut want = full;
        for y in 0..ph {
            want[(py + y) * w + px..(py + y) * w + px + pw].copy_from_slice(&part[y * pw..(y + 1) * pw]);
        }
        let got = read_back(&painter, &painter.textures[&id].tex);
        assert!(got == color_bytes(&want), "texture contents differ");
        drop(painter);
        unsafe { windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd) }.unwrap();
    }

    /// 整条画法在 WARP 上跑一遍：抖动与 egui.wgsl 的公式逐像素一致，剪裁、通道顺序、清屏色也对
    #[test]
    fn paints_like_egui_wgpu_on_warp() {
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
        let hwnd = test_window();
        let mut painter = Painter::new(hwnd, Driver::Warp, [64, 64]).unwrap();
        let id = TextureId::Managed(0);
        let mut delta = TexturesDelta::default();
        delta.set.entry(id).or_default().push(ImageDelta::full(ColorImage::new([2, 2], vec![Color32::from_rgb(128, 64, 192); 4]), TextureOptions::NEAREST));
        // 左边 48 列画一个方块，顶点色是灰的：乘出来不在整数级上，要靠抖动；剪裁只留上面 48 行
        let mut mesh = egui::epaint::Mesh::with_texture(id);
        mesh.add_rect_with_uv(Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(48.0, 64.0)), Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::from_rgb(128, 128, 128));
        let prims = [ClippedPrimitive { clip_rect: Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(64.0, 48.0)), primitive: Primitive::Mesh(mesh) }];
        let shot = painter.paint([0.2, 0.4, 0.6, 1.0], 1.0, &prims, &delta, true).unwrap().unwrap();
        drop(painter);
        unsafe { DestroyWindow(hwnd) }.unwrap();

        // fs_main_gamma_framebuffer 按 f32 算一遍；D3D 转 UNORM 是乘 255 加 0.5 截断
        let fract = |v: f32| v - v.floor();
        let noise = |x: f32, y: f32| (fract(52.982_918 * fract(0.067_110_56 * x + 0.005_837_15 * y)) - 0.5) * 0.95;
        let level = |c: u8, t: u8, n: f32| ((f32::from(c) / 255.0 * (f32::from(t) / 255.0) + n / 255.0).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let (mut bad, mut up) = (0, 0);
        for y in 0..64 {
            for x in 0..64 {
                let want = if x < 48 && y < 48 {
                    let n = noise(x as f32 + 0.5, y as f32 + 0.5);
                    [level(128, 128, n), level(128, 64, n), level(128, 192, n), 255]
                } else {
                    [51, 102, 153, 255]
                };
                up += usize::from(x < 48 && y < 48 && want[0] == 65);
                let p = shot.pixels[y * 64 + x];
                bad += usize::from([p.r(), p.g(), p.b(), p.a()] != want);
            }
        }
        // 红色 64.25 级：约四分之一抖到 65；不抖动的话这些全是 64
        assert!((300..900).contains(&up), "{up}");
        assert!(bad <= 4, "{bad} pixels differ from egui-wgpu's shader");
    }
}
