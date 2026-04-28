@group(0) @binding(0) var<uniform> vp: mat4x4<f32>;

struct PerDraw {
    model: mat4x4<f32>,
    color: vec3<f32>,
}

@group(1) @binding(0) var<uniform> per_draw: PerDraw;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vp * per_draw.model * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = in.color * per_draw.color;
    return vec4<f32>(color, 1.0);
}