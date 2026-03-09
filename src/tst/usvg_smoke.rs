use lyon_geom::{Point, point};
use lyon_tessellation::path::Path;
use lyon_tessellation::{BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers};
use tiny_skia_path::PathSegment;
use usvg::{Group, Node, Options, Paint, Tree};

const INLINE_SVG: &str = r##"
<svg width="64" height="64" viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <!-- Sun gradient -->
    <linearGradient id="retroGrad" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ff2ea6"/>
      <stop offset="100%" stop-color="#ff8c42"/>
    </linearGradient>

    <!-- Mask for sun stripes -->
    <mask id="stripeMask">
      <rect width="64" height="64" fill="white"/>
      <rect x="0" y="26" width="64" height="3" fill="black"/>
      <rect x="0" y="32" width="64" height="3" fill="black"/>
      <rect x="0" y="38" width="64" height="3" fill="black"/>
      <rect x="0" y="44" width="64" height="3" fill="black"/>
    </mask>

    <!-- Background gradient -->
    <linearGradient id="bgGrad" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#12002b"/>
      <stop offset="100%" stop-color="#1a003f"/>
    </linearGradient>
  </defs>

  <!-- Background -->
  <rect width="64" height="64" fill="url(#bgGrad)"/>

  <!-- Sun -->
  <circle cx="32" cy="32" r="20" fill="url(#retroGrad)" mask="url(#stripeMask)"/>
</svg>"##;

pub fn run() {
    usvg_lyon_mesh();
    /*
    let options = usvg::Options::default();
    match usvg::Tree::from_str(INLINE_SVG, &options) {
        Ok(tree) => {
            let size = tree.size();
            crate::log!(
                "usvg-smoke: ok width={} height={}\n",
                size.width(),
                size.height()
            );
        }
        Err(err) => {
            crate::log!("usvg-smoke: err {}\n", err);
        }
    }*/
}

pub fn usvg_lyon_mesh() {
    let opt = Options::default();
    let tree = match Tree::from_str(INLINE_SVG, &opt) {
        Ok(tree) => tree,
        Err(err) => {
            crate::log!("usvg-lyon-mesh: parse error {}\n", err);
            return;
        }
    };

    let mut tessellator = FillTessellator::new();

    fn walk_group(group: &Group, tessellator: &mut FillTessellator) {
        if group.mask().is_some() {
            crate::log!("usvg-lyon-mesh: group has mask applied\n");
        }

        for node in group.children() {
            match node {
                Node::Path(path) => {
                    let lyon_path = convert_path(path.data());
                    let mut buffers: VertexBuffers<Point<f32>, u16> = VertexBuffers::new();
                    if let Err(err) = tessellator.tessellate_path(
                        &lyon_path,
                        &FillOptions::default(),
                        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position()),
                    ) {
                        crate::log!("usvg-lyon-mesh: tessellation error {:?}\n", err);
                        continue;
                    }

                    crate::log!(
                        "usvg-lyon-mesh: generated {} vertices and {} indices\n",
                        buffers.vertices.len(),
                        buffers.indices.len()
                    );

                    if let Some(fill) = path.fill() {
                        match fill.paint() {
                            Paint::Color(c) => {
                                crate::log!(
                                    "usvg-lyon-mesh: solid fill rgb({}, {}, {})\n",
                                    c.red,
                                    c.green,
                                    c.blue
                                );
                            }
                            Paint::LinearGradient(g) => {
                                crate::log!(
                                    "usvg-lyon-mesh: linear gradient with {} stops\n",
                                    g.stops().len()
                                );
                            }
                            Paint::RadialGradient(g) => {
                                crate::log!(
                                    "usvg-lyon-mesh: radial gradient with {} stops\n",
                                    g.stops().len()
                                );
                            }
                            Paint::Pattern(_) => {
                                crate::log!("usvg-lyon-mesh: pattern fill\n");
                            }
                        }
                    }

                    if path.stroke().is_some() {
                        crate::log!("usvg-lyon-mesh: path has stroke\n");
                    }
                }
                Node::Group(group) => walk_group(group, tessellator),
                Node::Image(_) | Node::Text(_) => {}
            }
        }
    }

    walk_group(tree.root(), &mut tessellator);
}

fn convert_path(data: &tiny_skia_path::Path) -> Path {
    let mut builder = Path::builder();
    let mut subpath_open = false;

    for seg in data.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                if subpath_open {
                    builder.end(false);
                }
                builder.begin(point(p.x, p.y));
                subpath_open = true;
            }
            PathSegment::LineTo(p) => {
                if subpath_open {
                    builder.line_to(point(p.x, p.y));
                }
            }
            PathSegment::QuadTo(p1, p) => {
                if subpath_open {
                    builder.quadratic_bezier_to(point(p1.x, p1.y), point(p.x, p.y));
                }
            }
            PathSegment::CubicTo(p1, p2, p) => {
                if subpath_open {
                    builder.cubic_bezier_to(point(p1.x, p1.y), point(p2.x, p2.y), point(p.x, p.y));
                }
            }
            PathSegment::Close => {
                if subpath_open {
                    builder.end(true);
                    subpath_open = false;
                }
            }
        }
    }

    if subpath_open {
        builder.end(false);
    }

    builder.build()
}
