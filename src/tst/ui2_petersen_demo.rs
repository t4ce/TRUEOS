use alloc::vec::Vec;
use core::f64::consts::PI;

use embassy_time::{Duration as EmbassyDuration, Timer};
use kurbo::{Line, Point};
use petgraph::graph::UnGraph;
use petgraph::visit::EdgeRef;

const UI2_PETERSEN_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Petersen.get();
const UI2_PETERSEN_RT_W: u32 = 280;
const UI2_PETERSEN_RT_H: u32 = 280;
const UI2_PETERSEN_WINDOW_X: f32 = 640.0;
const UI2_PETERSEN_WINDOW_Y: f32 = 140.0;
const UI2_PETERSEN_WINDOW_Z: i16 = 32;
const UI2_PETERSEN_WINDOW_ALPHA: u8 = 236;
const UI2_PETERSEN_BG_RGBA: [u8; 4] = [0xF3, 0xF3, 0xF3, 0xFF];
const UI2_PETERSEN_EDGE_RGBA: [u8; 4] = [0x15, 0x15, 0x15, 0xFF];
const UI2_PETERSEN_OUTLINE_RGBA: [u8; 4] = [0x10, 0x10, 0x10, 0xFF];
const UI2_PETERSEN_OUTER_RADIUS: f64 = 0.43;
const UI2_PETERSEN_INNER_RADIUS: f64 = 0.18;
const UI2_PETERSEN_CANVAS_PAD: f64 = 22.0;
const UI2_PETERSEN_EDGE_STROKE_RADIUS: f64 = 1.75;
const UI2_PETERSEN_NODE_OUTLINE_RADIUS: f64 = 11.0;
const UI2_PETERSEN_NODE_FILL_RADIUS: f64 = 8.5;

#[derive(Clone, Copy)]
struct PetersenNode {
    position: Point,
    fill_rgba: [u8; 4],
}

fn petersen_outer_fill_rgba(index: usize) -> [u8; 4] {
    match index {
        0 => [0xFF, 0x25, 0x1E, 0xFF],
        1 => [0x2E, 0x73, 0xFF, 0xFF],
        2 => [0x12, 0xF0, 0x22, 0xFF],
        3 => [0xFF, 0x25, 0x1E, 0xFF],
        _ => [0x2E, 0x73, 0xFF, 0xFF],
    }
}

fn petersen_inner_fill_rgba(index: usize) -> [u8; 4] {
    match index {
        0 => [0x2E, 0x73, 0xFF, 0xFF],
        1 => [0xFF, 0x25, 0x1E, 0xFF],
        2 => [0xFF, 0x25, 0x1E, 0xFF],
        3 => [0x12, 0xF0, 0x22, 0xFF],
        _ => [0x12, 0xF0, 0x22, 0xFF],
    }
}

fn petersen_vertex(angle: f64, radius: f64) -> Point {
    Point::new(0.5 + libm::cos(angle) * radius, 0.5 + libm::sin(angle) * radius)
}

fn petersen_canvas_point(unit: Point, width: u32, height: u32) -> Point {
    let span_x = ((width as f64) - UI2_PETERSEN_CANVAS_PAD * 2.0).max(1.0);
    let span_y = ((height as f64) - UI2_PETERSEN_CANVAS_PAD * 2.0).max(1.0);
    Point::new(UI2_PETERSEN_CANVAS_PAD + unit.x * span_x, UI2_PETERSEN_CANVAS_PAD + unit.y * span_y)
}

fn build_petersen_graph(width: u32, height: u32) -> UnGraph<PetersenNode, ()> {
    let mut graph = UnGraph::<PetersenNode, ()>::new_undirected();
    let mut outer = [petgraph::graph::NodeIndex::new(0); 5];
    let mut inner = [petgraph::graph::NodeIndex::new(0); 5];
    let step = (2.0 * PI) / 5.0;
    let start_angle = -PI * 0.5;

    for index in 0..5 {
        let angle = start_angle + step * index as f64;
        let v0 =
            petersen_canvas_point(petersen_vertex(angle, UI2_PETERSEN_OUTER_RADIUS), width, height);
        outer[index] = graph.add_node(PetersenNode {
            position: v0,
            fill_rgba: petersen_outer_fill_rgba(index),
        });

        let v1 =
            petersen_canvas_point(petersen_vertex(angle, UI2_PETERSEN_INNER_RADIUS), width, height);
        inner[index] = graph.add_node(PetersenNode {
            position: v1,
            fill_rgba: petersen_inner_fill_rgba(index),
        });
    }

    for index in 0..5 {
        graph.add_edge(outer[index], outer[(index + 1) % 5], ());
        graph.add_edge(outer[index], inner[index], ());
        graph.add_edge(inner[index], inner[(index + 2) % 5], ());
    }

    graph
}

fn fill_rgba(pixels: &mut [u8], rgba: [u8; 4]) {
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&rgba);
    }
}

fn put_pixel_rgba(pixels: &mut [u8], width: u32, height: u32, x: i32, y: i32, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let offset = ((y as usize)
        .saturating_mul(width as usize)
        .saturating_add(x as usize))
    .saturating_mul(4);
    let Some(px) = pixels.get_mut(offset..offset.saturating_add(4)) else {
        return;
    };
    px.copy_from_slice(&rgba);
}

fn stroke_line_rgba(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    line: Line,
    stroke_radius: f64,
    rgba: [u8; 4],
) {
    let min_x = libm::floor(line.p0.x.min(line.p1.x) - stroke_radius - 1.0) as i32;
    let max_x = libm::ceil(line.p0.x.max(line.p1.x) + stroke_radius + 1.0) as i32;
    let min_y = libm::floor(line.p0.y.min(line.p1.y) - stroke_radius - 1.0) as i32;
    let max_y = libm::ceil(line.p0.y.max(line.p1.y) + stroke_radius + 1.0) as i32;
    let dx = line.p1.x - line.p0.x;
    let dy = line.p1.y - line.p0.y;
    let len_sq = dx * dx + dy * dy;
    let radius_sq = stroke_radius * stroke_radius;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let t = if len_sq > 0.0 {
                (((px - line.p0.x) * dx) + ((py - line.p0.y) * dy)) / len_sq
            } else {
                0.0
            }
            .clamp(0.0, 1.0);
            let nearest_x = line.p0.x + dx * t;
            let nearest_y = line.p0.y + dy * t;
            let dist_x = px - nearest_x;
            let dist_y = py - nearest_y;
            if (dist_x * dist_x) + (dist_y * dist_y) <= radius_sq {
                put_pixel_rgba(pixels, width, height, x, y, rgba);
            }
        }
    }
}

fn fill_circle_rgba(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    center: Point,
    radius: f64,
    rgba: [u8; 4],
) {
    let min_x = libm::floor(center.x - radius - 1.0) as i32;
    let max_x = libm::ceil(center.x + radius + 1.0) as i32;
    let min_y = libm::floor(center.y - radius - 1.0) as i32;
    let max_y = libm::ceil(center.y + radius + 1.0) as i32;
    let radius_sq = radius * radius;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = (x as f64 + 0.5) - center.x;
            let dy = (y as f64 + 0.5) - center.y;
            if (dx * dx) + (dy * dy) <= radius_sq {
                put_pixel_rgba(pixels, width, height, x, y, rgba);
            }
        }
    }
}

fn render_petersen_surface_rgba(width: u32, height: u32) -> (Vec<u8>, usize, usize) {
    let graph = build_petersen_graph(width, height);
    let mut pixels = vec![
        0u8;
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    ];
    fill_rgba(&mut pixels, UI2_PETERSEN_BG_RGBA);

    for edge in graph.edge_references() {
        let v0 = graph[edge.source()].position;
        let v1 = graph[edge.target()].position;
        let segment = Line::new(v0, v1);
        stroke_line_rgba(
            &mut pixels,
            width,
            height,
            segment,
            UI2_PETERSEN_EDGE_STROKE_RADIUS,
            UI2_PETERSEN_EDGE_RGBA,
        );
    }

    for node in graph.node_indices() {
        let style = graph[node];
        fill_circle_rgba(
            &mut pixels,
            width,
            height,
            style.position,
            UI2_PETERSEN_NODE_OUTLINE_RADIUS,
            UI2_PETERSEN_OUTLINE_RGBA,
        );
        fill_circle_rgba(
            &mut pixels,
            width,
            height,
            style.position,
            UI2_PETERSEN_NODE_FILL_RADIUS,
            style.fill_rgba,
        );
    }

    (pixels, graph.node_count(), graph.edge_count())
}

#[embassy_executor::task]
pub async fn ui2_petersen_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-petersen-demo");
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Petersen Graph",
        crate::r::ui2::Ui2Rect {
            x: UI2_PETERSEN_WINDOW_X,
            y: UI2_PETERSEN_WINDOW_Y,
            w: UI2_PETERSEN_RT_W as f32,
            h: UI2_PETERSEN_RT_H as f32,
        },
        UI2_PETERSEN_WINDOW_Z,
        UI2_PETERSEN_WINDOW_ALPHA,
        UI2_PETERSEN_TEX_ID,
        false,
        UI2_PETERSEN_BG_RGBA,
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task("ui2-petersen-demo");

    Timer::after(EmbassyDuration::from_millis(1)).await;

    let (surface_w, surface_h) = surface.size();
    let (pixels, node_count, edge_count) = render_petersen_surface_rgba(surface_w, surface_h);
    if !surface.upload_rgba(pixels.as_slice(), "ui2-petersen-demo") {
        crate::log!(
            "ui2-petersen-demo: upload failed window={} tex={} size={}x{}\n",
            surface.window_id(),
            surface.tex_id(),
            surface_w,
            surface_h
        );
        return;
    }
    let _ = crate::r::ui2::request_window_content_present(
        surface.window_id(),
        "ui2-petersen-demo-ready",
    );
    crate::log!(
        "ui2-petersen-demo: window={} tex={} size={}x{} nodes={} edges={}\n",
        surface.window_id(),
        surface.tex_id(),
        surface_w,
        surface_h,
        node_count,
        edge_count
    );

    loop {
        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-petersen-demo", 3_600_000).await {
            break;
        }
    }
}
