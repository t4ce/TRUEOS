//! Runtime CPU port of the studio's neutral-bias convolved membrane builder.
//! Float32 field samples, conservative quadtree merging, conforming shared
//! edges, threshold clipping and vertex welding match the debug JS mesher.
use alloc::{collections::BTreeMap, vec, vec::Vec};
type Point = [f64; 2];
type Sample = [f64; 3];
#[derive(Clone, Copy, Debug)]
pub struct Recipe {
    pub sigma: f64,
    pub level: f64,
    pub balance: f64,
    pub detail: usize,
    pub prune: bool,
}
pub struct Mesh {
    pub positions: Vec<Point>,
    pub indices: Vec<u32>,
}
#[derive(Clone, Copy)]
struct Kernel {
    point: Point,
    mass: f64,
}
fn distance2(a: Point, b: Point) -> f64 {
    (a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1])
}
fn gaussian(q2: f64) -> f64 {
    if q2 >= 12.25 {
        return 0.0;
    }
    let value = libm::exp(-q2 * 0.5);
    if q2 <= 9.0 {
        return value;
    }
    let t = (libm::sqrt(q2) - 3.0) / 0.5;
    value * (1.0 - t * t * (3.0 - 2.0 * t))
}
fn kernels(points: &[Point], recipe: Recipe, reach: f64) -> Vec<Kernel> {
    let mut degree = vec![0.0; points.len()];
    let mut output = Vec::new();
    for a in 0..points.len() {
        for b in a + 1..points.len() {
            let d2 = distance2(points[a], points[b]);
            let d = libm::sqrt(d2);
            if d >= reach || d < 1e-8 {
                continue;
            }
            let mid = [
                (points[a][0] + points[b][0]) * 0.5,
                (points[a][1] + points[b][1]) * 0.5,
            ];
            if recipe.prune
                && points
                    .iter()
                    .enumerate()
                    .any(|(k, &p)| k != a && k != b && distance2(p, mid) <= d2 / 4.0 + 1e-9)
            {
                continue;
            }
            let t = ((reach - d) / 0.30).clamp(0.0, 1.0);
            let weight = t * t * (3.0 - 2.0 * t);
            degree[a] += weight;
            degree[b] += weight;
            let steps = (libm::ceil(d / 0.22f64.min(recipe.sigma * 0.55)) as usize).max(2);
            let mass = weight * d
                / (steps as f64 * libm::sqrt(2.0 * core::f64::consts::PI) * recipe.sigma);
            if mass <= 1e-5 {
                continue;
            }
            for i in 0..steps {
                let t = (i as f64 + 0.5) / steps as f64;
                output.push(Kernel {
                    point: [
                        points[a][0] + (points[b][0] - points[a][0]) * t,
                        points[a][1] + (points[b][1] - points[a][1]) * t,
                    ],
                    mass,
                });
            }
        }
    }
    for (i, &point) in points.iter().enumerate() {
        let mass = (1.0 - degree[i] * 0.5).max(0.0);
        if mass > 1e-5 {
            output.push(Kernel { point, mass });
        }
    }
    if recipe.balance > 0.0 {
        let original: Vec<_> = output.iter().map(|k| k.mass).collect();
        let radius = 3.5 * recipe.sigma;
        for i in 0..output.len() {
            let mut density = 0.0;
            for j in 0..output.len() {
                let a = output[i].point;
                let b = output[j].point;
                if (a[0] - b[0]).abs() > radius || (a[1] - b[1]).abs() > radius {
                    continue;
                }
                density += original[j] * gaussian(distance2(a, b) / (recipe.sigma * recipe.sigma));
            }
            output[i].mass = original[i] / libm::pow(density.max(0.10), recipe.balance);
        }
    }
    output
}
struct Field {
    values: Vec<f32>,
    nx: usize,
    ny: usize,
    step: f64,
    x0: f64,
    y0: f64,
}
impl Field {
    fn value(&self, x: usize, y: usize) -> f64 {
        self.values[y * (self.nx + 1) + x] as f64
    }
    fn sample(&self, x: f64, y: f64) -> Sample {
        let ix = x as usize;
        let iy = y as usize;
        let fx = x - ix as f64;
        let fy = y - iy as f64;
        let a = self.value(ix, iy);
        let b = self.value((ix + 1).min(self.nx), iy);
        let c = self.value(ix, (iy + 1).min(self.ny));
        let d = self.value((ix + 1).min(self.nx), (iy + 1).min(self.ny));
        [
            x,
            y,
            (a * (1.0 - fx) + b * fx) * (1.0 - fy) + (c * (1.0 - fx) + d * fx) * fy,
        ]
    }
    fn point(&self, x: usize, y: usize) -> Sample {
        [x as f64, y as f64, self.value(x, y)]
    }
}
#[derive(Clone, Copy)]
struct Leaf {
    x: usize,
    y: usize,
    size: usize,
}
fn split(sums: &[u32], stride: usize, leaf: Leaf, leaves: &mut Vec<Leaf>) {
    let Leaf { x, y, size } = leaf;
    let count = sums[(y + size + 1) * stride + x + size + 1] as i64
        - sums[y * stride + x + size + 1] as i64
        - sums[(y + size + 1) * stride + x] as i64
        + sums[y * stride + x] as i64;
    if count == 0 {
        return;
    }
    if size == 1 || count == ((size + 1) * (size + 1)) as i64 {
        leaves.push(leaf);
        return;
    }
    let h = size / 2;
    for (xx, yy) in [(x, y), (x + h, y), (x, y + h), (x + h, y + h)] {
        split(
            sums,
            stride,
            Leaf {
                x: xx,
                y: yy,
                size: h,
            },
            leaves,
        );
    }
}
struct Builder<'a> {
    field: &'a Field,
    level: f64,
    mesh: Mesh,
    vertices: BTreeMap<(i64, i64), u32>,
}
impl Builder<'_> {
    fn vertex(&mut self, p: Sample) -> u32 {
        let key = (libm::round(p[0] * 1e8) as i64, libm::round(p[1] * 1e8) as i64);
        if let Some(&index) = self.vertices.get(&key) {
            return index;
        }
        let index = self.mesh.positions.len() as u32;
        self.mesh.positions.push([
            self.field.x0 + p[0] * self.field.step,
            self.field.y0 + p[1] * self.field.step,
        ]);
        self.vertices.insert(key, index);
        index
    }
    fn emit(&mut self, a: Sample, b: Sample, c: Sample) {
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-10 {
            return;
        }
        let (ai, bi, ci) = (self.vertex(a), self.vertex(b), self.vertex(c));
        if ai != bi && bi != ci && ci != ai {
            self.mesh.indices.extend_from_slice(&[ai, bi, ci]);
        }
    }
    fn clip(&mut self, a: Sample, b: Sample, c: Sample) {
        let source = [a, b, c];
        let mut polygon = [[0.0; 3]; 4];
        let mut count: usize = 0;
        for i in 0..3 {
            let u = source[i];
            let v = source[(i + 1) % 3];
            let inside = u[2] >= self.level;
            let other = v[2] >= self.level;
            if inside {
                polygon[count] = u;
                count += 1;
            }
            if inside != other {
                let t = (self.level - u[2]) / (v[2] - u[2]);
                polygon[count] = [
                    u[0] + (v[0] - u[0]) * t,
                    u[1] + (v[1] - u[1]) * t,
                    self.level,
                ];
                count += 1;
            }
        }
        for i in 1..count.saturating_sub(1) {
            self.emit(polygon[0], polygon[i], polygon[i + 1]);
        }
    }
}
pub fn build(
    points: &[Point],
    width: f64,
    recipe: Recipe,
    reach: f64,
) -> Result<Mesh, &'static str> {
    if ![6, 12, 24, 40].contains(&recipe.detail)
        || !(0.1..=2.0).contains(&recipe.sigma)
        || !(0.0..=1.0).contains(&recipe.balance)
        || !(0.1..=2.0).contains(&recipe.level)
        || !(0.1..=6.0).contains(&reach)
        || points.len() > 198
        || !(6.0..=18.0).contains(&width)
        || points.iter().flatten().any(|x| !x.is_finite())
    {
        return Err("invalid-membrane-recipe");
    }
    let kernels = kernels(points, recipe, reach);
    let radius = 3.5 * recipe.sigma;
    let pad = (radius + 0.25).max(1.0);
    let mut step = 1.0 / recipe.detail as f64;
    let target = (width + 2.0 * pad) * (11.0 + 2.0 * pad) / (step * step);
    if target > 420000.0 {
        step *= libm::sqrt(target / 420000.0);
    }
    let nx = (libm::ceil((width + 2.0 * pad) / step / 16.0) as usize).max(1) * 16;
    let ny = (libm::ceil((11.0 + 2.0 * pad) / step / 16.0) as usize).max(1) * 16;
    let mut field = Field {
        values: vec![0.0; (nx + 1) * (ny + 1)],
        nx,
        ny,
        step,
        x0: width / 2.0 - nx as f64 * step / 2.0,
        y0: 5.5 - ny as f64 * step / 2.0,
    };
    for k in &kernels {
        let bound = |v: f64, origin: f64, max: usize, ceil: bool| -> usize {
            let q = (v - origin) / step;
            (if ceil { libm::ceil(q) } else { libm::floor(q) }).clamp(0.0, max as f64) as usize
        };
        let ax = bound(k.point[0] - radius, field.x0, nx, false);
        let bx = bound(k.point[0] + radius, field.x0, nx, true);
        let ay = bound(k.point[1] - radius, field.y0, ny, false);
        let by = bound(k.point[1] + radius, field.y0, ny, true);
        for y in ay..=by {
            for x in ax..=bx {
                let p = [field.x0 + x as f64 * step, field.y0 + y as f64 * step];
                let v = k.mass * gaussian(distance2(p, k.point) / (recipe.sigma * recipe.sigma));
                let at = y * (nx + 1) + x;
                field.values[at] = (field.values[at] as f64 + v) as f32;
            }
        }
    }
    let stride = nx + 2;
    let mut sums = vec![0u32; (nx + 2) * (ny + 2)];
    for y in 0..=ny {
        let mut row = 0;
        for x in 0..=nx {
            row += u32::from(field.value(x, y) >= recipe.level);
            sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row;
        }
    }
    let mut leaves = Vec::new();
    for y in (0..ny).step_by(16) {
        for x in (0..nx).step_by(16) {
            split(&sums, stride, Leaf { x, y, size: 16 }, &mut leaves);
        }
    }
    drop(sums);
    let mut horizontal = vec![Vec::new(); ny + 1];
    let mut vertical = vec![Vec::new(); nx + 1];
    for &Leaf { x, y, size: s } in &leaves {
        for (xx, yy) in [(x, y), (x + s, y), (x + s, y + s), (x, y + s)] {
            horizontal[yy].push(xx);
            vertical[xx].push(yy);
        }
    }
    for row in horizontal.iter_mut().chain(vertical.iter_mut()) {
        row.sort_unstable();
        row.dedup();
    }
    let mut builder = Builder {
        field: &field,
        level: recipe.level,
        mesh: Mesh {
            positions: Vec::new(),
            indices: Vec::new(),
        },
        vertices: BTreeMap::new(),
    };
    let mut ring = Vec::new();
    for Leaf { x, y, size: s } in leaves {
        ring.clear();
        for &t in &horizontal[y] {
            if t >= x && t < x + s {
                ring.push(field.point(t, y));
            }
        }
        for &t in &vertical[x + s] {
            if t >= y && t < y + s {
                ring.push(field.point(x + s, t));
            }
        }
        for &t in horizontal[y + s].iter().rev() {
            if t > x && t <= x + s {
                ring.push(field.point(t, y + s));
            }
        }
        for &t in vertical[x].iter().rev() {
            if t > y && t <= y + s {
                ring.push(field.point(x, t));
            }
        }
        let center = field.sample(x as f64 + s as f64 / 2.0, y as f64 + s as f64 / 2.0);
        for i in 0..ring.len() {
            builder.clip(center, ring[i], ring[(i + 1) % ring.len()]);
        }
    }
    Ok(builder.mesh)
}
