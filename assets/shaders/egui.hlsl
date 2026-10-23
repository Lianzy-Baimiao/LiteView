// egui 的 Direct3D 11 着色器：照搬 egui-wgpu 0.36.2 的 egui.wgsl（顶点同样布局；片元走 fs_main_gamma_framebuffer，
// 输出到非 sRGB 的 B8G8R8A8_UNORM，开抖动），画出来与 wgpu 后端逐像素一致。
// 改了之后运行 `cargo run --release --example compile_shaders` 重新生成 assets/shaders/*.dxbc 和 shaders.rs。

cbuffer Locals : register(b0) {
    float2 screen_size;   // 逻辑点
    uint dithering;       // 1 = 抖动
    uint predictable;     // 没用到（egui-wgpu 的测试选项，默认 0）
};

struct VsIn {
    float2 pos : POSITION;
    float2 uv : TEXCOORD0;
    uint color : COLOR0;  // [u8; 4] sRGB，按 u32 读（与 wgsl 一样手动拆）
};

struct VsOut {
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;  // gamma 0-1
    float4 pos : SV_POSITION;
};

float4 unpack_color(uint c) {
    return float4(float(c & 255u), float((c >> 8u) & 255u), float((c >> 16u) & 255u), float((c >> 24u) & 255u)) / 255.0;
}

VsOut vs_main(VsIn i) {
    VsOut o;
    o.uv = i.uv;
    o.color = unpack_color(i.color);
    o.pos = float4(2.0 * i.pos.x / screen_size.x - 1.0, 1.0 - 2.0 * i.pos.y / screen_size.y, 0.0, 1.0);
    return o;
}

Texture2D tex : register(t0);
SamplerState samp : register(s0);

// Jimenez 2014 interleaved gradient noise（与 egui.wgsl 相同）
float interleaved_gradient_noise(float2 n) {
    float f = 0.06711056 * n.x + 0.00583715 * n.y;
    return frac(52.9829189 * frac(f));
}

float3 dither_interleaved(float3 rgb, float levels, float4 frag_coord) {
    float noise = interleaved_gradient_noise(frag_coord.xy);
    noise = (noise - 0.5) * 0.95;
    return rgb + noise / (levels - 1.0);
}

float4 ps_main(VsOut i) : SV_TARGET {
    float4 tex_gamma = tex.Sample(samp, i.uv);
    float4 out_gamma = i.color * tex_gamma;
    if (dithering == 1u) {
        out_gamma = float4(dither_interleaved(out_gamma.rgb, 256.0, i.pos), out_gamma.a);
    }
    return out_gamma;
}
