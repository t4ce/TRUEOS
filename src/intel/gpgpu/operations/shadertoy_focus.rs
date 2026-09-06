// Generic screen-space density control; positions use top-left output pixels.
// The C++ adapter's foveated.clcpp implements the matching radial map/resolve.
#[derive(Copy, Clone, Debug)]
struct ShaderToyFocusPlan {
    width: u32,
    height: u32,
    focus: [f32; 4],
}

fn shadertoy_focus_plan(
    width: u32,
    height: u32,
    params: ShaderToyFrameParams,
) -> Option<ShaderToyFocusPlan> {
    const SAMPLE_PIXELS: f64 = 1280.0 * 720.0;
    let area = f64::from(width) * f64::from(height);
    if width < 2
        || height < 2
        || area <= SAMPLE_PIXELS
        || params.shader_id != SHADERTOY_SHADER_PROTEAN_CLOUDS
        || params.flags & SHADERTOY_FLAG_NATIVE_RESOLUTION != 0
    {
        return None;
    }
    // Continuous density while resizing: native below 720p, at most 2x per
    // axis. Ceil ensures odd dimensions include the complete image. At 1440p
    // exactly 1280x720 cloud evaluations, with 1:1 pitch at the focus center.
    let scale = libm::sqrt(area / SAMPLE_PIXELS).min(2.0);
    let sw = (libm::ceil(f64::from(width) / scale) as u32).clamp(1, width);
    let sh = (libm::ceil(f64::from(height) / scale) as u32).clamp(1, height);
    let boost = (width as f32 / sw as f32)
        .min(height as f32 / sh as f32)
        .clamp(1.0, 2.0);
    Some(ShaderToyFocusPlan {
        width: sw,
        height: sh,
        focus: shadertoy_protean_focus(width, height, params, boost),
    })
}

/// Project the tunnel centerline eight world units ahead through the shader's
/// own camera, including its final ray rotation and mouse displacement. This
/// is a geometric focus estimate, not an image-feature or gaze tracker.
fn shadertoy_protean_focus(
    width: u32,
    height: u32,
    params: ShaderToyFrameParams,
    boost: f32,
) -> [f32; 4] {
    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }
    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
    fn norm(a: [f32; 3]) -> [f32; 3] {
        let s = 1.0 / libm::sqrtf(dot(a, a));
        [a[0] * s, a[1] * s, a[2] * s]
    }
    fn disp(z: f32) -> [f32; 3] {
        [2.0 * libm::sinf(z * 0.22), 2.0 * libm::cosf(z * 0.175), z]
    }
    let (w, h, time) = (width as f32, height as f32, params.time_seconds);
    let z = time * 3.0;
    let bsx = (params.mouse_x - 0.5 * w) / h;
    let mut ro = disp(z);
    ro[0] = ro[0] * 0.85 + libm::sinf(time) * 0.5;
    ro[1] *= 0.85;
    let mut aim = disp(z + 3.5);
    aim[0] *= 0.85;
    aim[1] *= 0.85;
    let target = norm(sub(ro, aim));
    let right = norm(cross(target, [0.0, 1.0, 0.0]));
    let up = norm(cross(right, target));
    let right = norm(cross(up, target));
    ro[0] -= bsx * 2.0;
    let direction = sub(disp(z + 8.0), ro);
    let angle = -disp(z + 3.5)[0] * 0.2 + bsx;
    let (c, s) = (libm::cosf(angle), libm::sinf(angle));
    let direction = [
        direction[0] * c - direction[1] * s,
        direction[0] * s + direction[1] * c,
        direction[2],
    ];
    let depth = -dot(direction, target);
    let (mut cx, mut cy) = (w * 0.5, h * 0.5);
    if depth > 0.01 {
        let px = cx + h * dot(direction, right) / depth;
        let py = cy - h * dot(direction, up) / depth;
        if px.is_finite() && py.is_finite() {
            cx = px;
            cy = py;
        }
    }
    cx = cx.clamp(w * 0.15, w * 0.85);
    cy = cy.clamp(h * 0.15, h * 0.85);
    // The compact warp is identity outside this disk. Keeping it inside the
    // viewport guarantees complete edge coverage, even for an offscreen focus.
    let radius = (h * 0.48).min(cx).min(w - cx).min(cy).min(h - cy);
    [cx, cy, radius, boost]
}

#[derive(Copy, Clone, Debug)]
struct ShaderToyPass {
    phase: u32, // 0 native, 1 shade sample atlas, 2 reconstruct full output
    width: u32,
    height: u32,
    source: GpgpuRgba8Surface,
    focus: [f32; 4],
}

impl ShaderToyPass {
    fn native(dst: GpgpuRgba8Surface) -> Self {
        Self {
            phase: 0,
            width: dst.width,
            height: dst.height,
            source: dst,
            focus: [0.0, 0.0, 1.0, 1.0],
        }
    }
}
