//! 把 assets/shaders/egui.hlsl 编译成 DXBC（运行时直接用编好的，不依赖 d3dcompiler）：
//! `cargo run --release --example compile_shaders`。只在 Windows 上有用。

#[cfg(windows)]
fn main() {
    use windows::Win32::Graphics::Direct3D::Fxc::{D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_IEEE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompile};
    use windows::Win32::Graphics::Direct3D::ID3DBlob;
    use windows::core::{PCSTR, s};

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/shaders");
    let src = std::fs::read_to_string(dir.join("egui.hlsl")).expect("egui.hlsl");
    let src = src.replace("\r\n", "\n");
    let compile = |entry: PCSTR, target: PCSTR| -> Vec<u8> {
        let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
        // IEEE strictness：抖动噪声按 wgsl 那样逐步算，不让编译器重排浮点运算
        let flags = D3DCOMPILE_OPTIMIZATION_LEVEL3 | D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_IEEE_STRICTNESS;
        let r = unsafe { D3DCompile(src.as_ptr().cast(), src.len(), s!("egui.hlsl"), None, None, entry, target, flags, 0, &mut code, Some(&mut errors)) };
        if let Err(e) = r {
            let msg = errors.map(|b| unsafe { String::from_utf8_lossy(std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).into_owned() });
            panic!("compile failed: {e} {}", msg.unwrap_or_default());
        }
        let b = code.expect("bytecode");
        unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec() }
    };
    let vs = compile(s!("vs_main"), s!("vs_4_0"));
    let ps = compile(s!("ps_main"), s!("ps_4_0"));
    std::fs::write(dir.join("egui_vs.dxbc"), &vs).expect("write vs");
    std::fs::write(dir.join("egui_ps.dxbc"), &ps).expect("write ps");
    // 源码的哈希：单元测试据此发现「改了 hlsl 却没重新编译」
    let hash = src.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    std::fs::write(dir.join("egui.hash"), format!("{hash:016x}\n")).expect("write hash");
    println!("vs {} bytes, ps {} bytes, source {hash:016x}", vs.len(), ps.len());
}

#[cfg(not(windows))]
fn main() {}
