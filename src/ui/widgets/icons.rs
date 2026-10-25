use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Ui};

/// The Lucide icons the handout uses, drawn as strokes so no image loader is
/// needed. Coordinates are Lucide's own 24-unit grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Search,
    ChevronDown,
    ChevronRight,
    ArrowLeft,
    ArrowRight,
    Refresh,
    ExternalLink,
    FolderOpen,
    Calculator,
    Globe,
    Terminal,
    Monitor,
    Lightbulb,
    Close,
    Check,
    Keyboard,
    LayoutGrid,
    RotateCcw,
    Ban,
    TriangleAlert,
    Ellipsis,
    CircleHelp,
    Settings,
    Sparkles,
    Clock,
}

struct Path {
    points: Vec<Pos2>,
}

impl Path {
    fn start(x: f32, y: f32) -> Self {
        Self {
            points: vec![pos2(x, y)],
        }
    }

    fn line(mut self, x: f32, y: f32) -> Self {
        self.points.push(pos2(x, y));
        self
    }

    /// Lucide rounds its corners with small arcs; a quadratic curve through the
    /// corner point is indistinguishable at 13 to 16 px.
    fn corner(mut self, cx: f32, cy: f32, x: f32, y: f32) -> Self {
        let from = *self.points.last().expect("a path always has a start");
        let control = pos2(cx, cy);
        let to = pos2(x, y);
        for step in 1..=6 {
            let t = step as f32 / 6.0;
            let a = from.lerp(control, t);
            let b = control.lerp(to, t);
            self.points.push(a.lerp(b, t));
        }
        self
    }

    fn arc(centre: Pos2, radius: f32, from_degrees: f32, to_degrees: f32) -> Self {
        let steps = 40;
        let points = (0..=steps)
            .map(|step| {
                let t = step as f32 / steps as f32;
                let angle = (from_degrees + (to_degrees - from_degrees) * t).to_radians();
                pos2(
                    centre.x + radius * angle.cos(),
                    centre.y + radius * angle.sin(),
                )
            })
            .collect();
        Self { points }
    }

    /// Continues with the points of another path, as when an arc closes a
    /// bulb's outline.
    fn then(mut self, other: Path) -> Self {
        self.points.extend(other.points);
        self
    }

    fn ellipse(centre: Pos2, radius_x: f32, radius_y: f32) -> Self {
        let mut path = Self::arc(centre, 1.0, 0.0, 360.0);
        for point in &mut path.points {
            *point = pos2(
                centre.x + (point.x - centre.x) * radius_x,
                centre.y + (point.y - centre.y) * radius_y,
            );
        }
        path
    }
}

fn paths(icon: Icon) -> Vec<Path> {
    match icon {
        Icon::Search => vec![
            Path::arc(pos2(11.0, 11.0), 8.0, 0.0, 360.0),
            Path::start(21.0, 21.0).line(16.7, 16.7),
        ],
        Icon::ChevronDown => vec![Path::start(6.0, 9.0).line(12.0, 15.0).line(18.0, 9.0)],
        Icon::ChevronRight => vec![Path::start(9.0, 18.0).line(15.0, 12.0).line(9.0, 6.0)],
        Icon::ArrowLeft => vec![
            Path::start(12.0, 19.0).line(5.0, 12.0).line(12.0, 5.0),
            Path::start(19.0, 12.0).line(5.0, 12.0),
        ],
        Icon::ArrowRight => vec![
            Path::start(5.0, 12.0).line(19.0, 12.0),
            Path::start(12.0, 5.0).line(19.0, 12.0).line(12.0, 19.0),
        ],
        // The handout's refresh-cw: a 312° arc with an arrow head at the top right.
        Icon::Refresh => vec![
            Path::arc(pos2(12.0, 12.0), 9.0, 0.0, 312.0),
            Path::start(21.0, 3.0).line(21.0, 9.0).line(15.0, 9.0),
        ],
        Icon::ExternalLink => vec![
            Path::start(15.0, 3.0).line(21.0, 3.0).line(21.0, 9.0),
            Path::start(10.0, 14.0).line(21.0, 3.0),
            Path::start(18.0, 13.0)
                .line(18.0, 19.0)
                .corner(18.0, 21.0, 16.0, 21.0)
                .line(5.0, 21.0)
                .corner(3.0, 21.0, 3.0, 19.0)
                .line(3.0, 8.0)
                .corner(3.0, 6.0, 5.0, 6.0)
                .line(11.0, 6.0),
        ],
        Icon::FolderOpen => vec![Path::start(6.0, 14.0)
            .line(7.5, 11.1)
            .corner(8.07, 10.0, 9.24, 10.0)
            .line(20.0, 10.0)
            .corner(22.58, 10.0, 21.94, 12.5)
            .line(20.4, 18.5)
            .corner(20.0, 20.0, 18.45, 20.0)
            .line(4.0, 20.0)
            .corner(2.0, 20.0, 2.0, 18.0)
            .line(2.0, 5.0)
            .corner(2.0, 3.0, 4.0, 3.0)
            .line(7.9, 3.0)
            .corner(8.98, 3.0, 9.59, 3.9)
            .line(10.4, 5.1)
            .corner(11.01, 6.0, 12.07, 6.0)
            .line(18.0, 6.0)
            .corner(20.0, 6.0, 20.0, 8.0)
            .line(20.0, 10.0)],
        // Lucide's calculator, with its key dots drawn as short dashes: a
        // path of one point draws nothing.
        Icon::Calculator => {
            let mut paths = vec![
                Path::start(6.0, 2.0)
                    .line(18.0, 2.0)
                    .corner(20.0, 2.0, 20.0, 4.0)
                    .line(20.0, 20.0)
                    .corner(20.0, 22.0, 18.0, 22.0)
                    .line(6.0, 22.0)
                    .corner(4.0, 22.0, 4.0, 20.0)
                    .line(4.0, 4.0)
                    .corner(4.0, 2.0, 6.0, 2.0),
                Path::start(8.0, 6.0).line(16.0, 6.0),
            ];
            for y in [10.0, 14.0, 18.0] {
                for x in [8.0, 12.0, 16.0] {
                    paths.push(Path::start(x - 0.5, y).line(x + 0.5, y));
                }
            }
            paths
        }
        Icon::Terminal => vec![
            Path::start(4.0, 17.0).line(10.0, 11.0).line(4.0, 5.0),
            Path::start(12.0, 19.0).line(20.0, 19.0),
        ],
        Icon::Monitor => vec![
            Path::start(4.0, 4.0)
                .line(20.0, 4.0)
                .corner(22.0, 4.0, 22.0, 6.0)
                .line(22.0, 15.0)
                .corner(22.0, 17.0, 20.0, 17.0)
                .line(4.0, 17.0)
                .corner(2.0, 17.0, 2.0, 15.0)
                .line(2.0, 6.0)
                .corner(2.0, 4.0, 4.0, 4.0),
            Path::start(8.0, 21.0).line(16.0, 21.0),
            Path::start(12.0, 17.0).line(12.0, 21.0),
        ],
        // Lucide's lightbulb: the glass outline, then the two base strokes.
        Icon::Lightbulb => vec![
            Path::start(15.0, 14.0)
                .corner(15.4, 12.6, 16.5, 11.5)
                .corner(17.9, 10.2, 18.0, 8.0)
                .then(Path::arc(pos2(12.0, 8.0), 6.0, 0.0, -180.0))
                .corner(6.1, 10.2, 7.5, 11.5)
                .corner(8.6, 12.6, 9.0, 14.0),
            Path::start(9.0, 18.0).line(15.0, 18.0),
            Path::start(10.0, 22.0).line(14.0, 22.0),
        ],
        Icon::Keyboard => {
            let mut paths = vec![
                Path::start(4.0, 4.0)
                    .line(20.0, 4.0)
                    .corner(22.0, 4.0, 22.0, 6.0)
                    .line(22.0, 18.0)
                    .corner(22.0, 20.0, 20.0, 20.0)
                    .line(4.0, 20.0)
                    .corner(2.0, 20.0, 2.0, 18.0)
                    .line(2.0, 6.0)
                    .corner(2.0, 4.0, 4.0, 4.0),
                Path::start(7.0, 16.0).line(17.0, 16.0),
            ];
            for (x, y) in [
                (6.0, 8.0),
                (10.0, 8.0),
                (14.0, 8.0),
                (18.0, 8.0),
                (8.0, 12.0),
                (12.0, 12.0),
                (16.0, 12.0),
            ] {
                paths.push(Path::start(x - 0.5, y).line(x + 0.5, y));
            }
            paths
        }
        Icon::LayoutGrid => [(3.0, 3.0), (14.0, 3.0), (14.0, 14.0), (3.0, 14.0)]
            .into_iter()
            .map(|(x, y)| {
                Path::start(x + 1.0, y)
                    .line(x + 6.0, y)
                    .corner(x + 7.0, y, x + 7.0, y + 1.0)
                    .line(x + 7.0, y + 6.0)
                    .corner(x + 7.0, y + 7.0, x + 6.0, y + 7.0)
                    .line(x + 1.0, y + 7.0)
                    .corner(x, y + 7.0, x, y + 6.0)
                    .line(x, y + 1.0)
                    .corner(x, y, x + 1.0, y)
            })
            .collect(),
        // Lucide's rotate-ccw: most of a circle, the arrow head top left.
        Icon::RotateCcw => vec![
            Path::arc(pos2(12.0, 12.0), 9.0, 180.0, 500.0),
            Path::start(3.0, 3.0).line(3.0, 8.0).line(8.0, 8.0),
        ],
        Icon::Ban => vec![
            Path::arc(pos2(12.0, 12.0), 10.0, 0.0, 360.0),
            Path::start(4.9, 4.9).line(19.1, 19.1),
        ],
        Icon::TriangleAlert => vec![
            Path::start(10.3, 3.9)
                .line(1.8, 18.0)
                .corner(1.0, 20.0, 3.5, 21.0)
                .line(20.5, 21.0)
                .corner(23.0, 20.0, 22.2, 18.0)
                .line(13.7, 3.9)
                .corner(12.0, 1.5, 10.3, 3.9),
            Path::start(12.0, 9.0).line(12.0, 13.0),
            Path::start(11.5, 17.0).line(12.5, 17.0),
        ],
        Icon::Ellipsis => [5.0, 12.0, 19.0]
            .into_iter()
            .map(|x| Path::arc(pos2(x, 12.0), 1.0, 0.0, 360.0))
            .collect(),
        Icon::CircleHelp => vec![
            Path::arc(pos2(12.0, 12.0), 10.0, 0.0, 360.0),
            Path::start(9.1, 9.0)
                .corner(9.5, 6.5, 12.0, 6.5)
                .corner(15.0, 6.5, 15.0, 9.0)
                .corner(15.0, 11.0, 12.0, 12.0)
                .line(12.0, 13.5),
            Path::start(11.5, 17.0).line(12.5, 17.0),
        ],
        // Lucide's settings cog: eight teeth round a ring, a hole in the middle.
        Icon::Settings => {
            let centre = pos2(12.0, 12.0);
            let mut points = Vec::new();
            for tooth in 0..8 {
                let middle = tooth as f32 * 45.0;
                for (offset, radius) in [(-20.0, 7.6), (-10.0, 10.0), (10.0, 10.0), (20.0, 7.6)] {
                    let angle = (middle + offset).to_radians();
                    points.push(pos2(
                        centre.x + radius * angle.cos(),
                        centre.y + radius * angle.sin(),
                    ));
                }
            }
            points.push(points[0]);
            vec![Path { points }, Path::arc(centre, 3.0, 0.0, 360.0)]
        }
        // Lucide's sparkles: a four-pointed star with two small crosses.
        Icon::Sparkles => vec![
            Path::start(11.0, 2.5)
                .corner(12.5, 9.5, 19.5, 11.0)
                .corner(12.5, 12.5, 11.0, 19.5)
                .corner(9.5, 12.5, 2.5, 11.0)
                .corner(9.5, 9.5, 11.0, 2.5),
            Path::start(20.0, 3.0).line(20.0, 7.0),
            Path::start(18.0, 5.0).line(22.0, 5.0),
            Path::start(4.0, 17.0).line(4.0, 21.0),
            Path::start(2.0, 19.0).line(6.0, 19.0),
        ],
        Icon::Clock => vec![
            Path::arc(pos2(12.0, 12.0), 10.0, 0.0, 360.0),
            Path::start(12.0, 6.0).line(12.0, 12.0).line(16.0, 14.0),
        ],
        Icon::Close => vec![
            Path::start(18.0, 6.0).line(6.0, 18.0),
            Path::start(6.0, 6.0).line(18.0, 18.0),
        ],
        Icon::Check => vec![Path::start(20.0, 6.0).line(9.0, 17.0).line(4.0, 12.0)],
        Icon::Globe => vec![
            Path::arc(pos2(12.0, 12.0), 10.0, 0.0, 360.0),
            Path::ellipse(pos2(12.0, 12.0), 4.5, 10.0),
            Path::start(2.0, 12.0).line(22.0, 12.0),
        ],
    }
}

/// Lucide's stroke is 2 units wide on its 24-unit grid; it scales with the
/// icon rather than snapping, so a 13 px icon keeps its drawn weight.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, colour: Color32) {
    paint_weighted(painter, rect, icon, colour, 2.0);
}

/// The top and bottom colour of the tile behind an icon in themes with
/// icon tiles: one fixed pair per kind of thing the icon stands for.
pub fn tile_colours(icon: Icon) -> (Color32, Color32) {
    let pair = |top: u32, bottom: u32| (rgb(top), rgb(bottom));
    match icon {
        Icon::FolderOpen | Icon::ExternalLink | Icon::Search => pair(0x9aa3ad, 0x646d78),
        Icon::Check | Icon::Refresh => pair(0x47c98a, 0x1f955a),
        Icon::Terminal | Icon::Keyboard | Icon::Monitor => pair(0xad86f5, 0x7448d6),
        Icon::Calculator => pair(0x34c7bb, 0x128a83),
        Icon::Lightbulb => pair(0xf2bd68, 0xc4852a),
        Icon::Globe => pair(0x55b4f5, 0x1f73c8),
        _ => pair(0x5b9cff, 0x2f63d8),
    }
}

const fn rgb(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// `icon` in white on its coloured tile: a top-to-bottom gradient with
/// rounded corners, a faint light ring inside the edge and a soft shadow
/// under the glyph. `glyph` is the icon's own square inside `tile`.
pub fn paint_on_tile(painter: &Painter, tile: Rect, glyph: Rect, icon: Icon, radius: f32) {
    let (top, bottom) = tile_colours(icon);
    painter.add(gradient_rect(tile, radius, top, bottom));
    painter.rect_stroke(
        tile,
        radius.round() as u8,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 36)),
        StrokeKind::Inside,
    );
    let weight = if glyph.width() >= 32.0 { 1.5 } else { 2.0 };
    paint_weighted(
        painter,
        glyph.translate(vec2(0.0, 1.0)),
        icon,
        Color32::from_rgba_unmultiplied(0, 0, 0, 64),
        weight,
    );
    paint_weighted(painter, glyph, icon, Color32::WHITE, weight);
}

/// A rounded rectangle filled from `top` to `bottom`, as a fan of
/// triangles: egui fills shapes with one colour only.
fn gradient_rect(rect: Rect, radius: f32, top: Color32, bottom: Color32) -> Mesh {
    const STEPS: usize = 6;
    let radius = radius.clamp(0.0, rect.width().min(rect.height()) / 2.0);
    let corners = [
        (pos2(rect.right() - radius, rect.top() + radius), -90.0_f32),
        (pos2(rect.right() - radius, rect.bottom() - radius), 0.0),
        (pos2(rect.left() + radius, rect.bottom() - radius), 90.0),
        (pos2(rect.left() + radius, rect.top() + radius), 180.0),
    ];
    let colour_at = |y: f32| {
        let t = ((y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
        let channel =
            |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Color32::from_rgb(
            channel(top.r(), bottom.r()),
            channel(top.g(), bottom.g()),
            channel(top.b(), bottom.b()),
        )
    };
    let mut mesh = Mesh::default();
    let centre = rect.center();
    mesh.vertices.push(Vertex {
        pos: centre,
        uv: WHITE_UV,
        color: colour_at(centre.y),
    });
    for (corner, start) in corners {
        for step in 0..=STEPS {
            let angle = (start + 90.0 * step as f32 / STEPS as f32).to_radians();
            let pos = corner + vec2(angle.cos(), angle.sin()) * radius;
            mesh.vertices.push(Vertex {
                pos,
                uv: WHITE_UV,
                color: colour_at(pos.y),
            });
        }
    }
    let around = mesh.vertices.len() as u32 - 1;
    for index in 0..around {
        mesh.add_triangle(0, 1 + index, 1 + (index + 1) % around);
    }
    mesh
}

/// The same icon with a lighter or heavier line, in Lucide's 24-unit grid.
/// Large icons use 1.5, because 2 looks clumsy at 40 px.
pub fn paint_weighted(painter: &Painter, rect: Rect, icon: Icon, colour: Color32, weight: f32) {
    let scale = rect.width().min(rect.height()) / 24.0;
    let stroke = Stroke::new(weight * scale, colour);
    for path in paths(icon) {
        let points = path
            .points
            .into_iter()
            .map(|point| rect.min + point.to_vec2() * scale)
            .collect();
        painter.line(points, stroke);
    }
}

pub fn show(ui: &mut Ui, icon: Icon, size: f32, colour: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), rect, icon, colour);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gradient_tile_runs_from_the_top_colour_to_the_bottom_colour() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(24.0, 24.0));
        let (top, bottom) = tile_colours(Icon::Calculator);
        let mesh = gradient_rect(rect, 6.0, top, bottom);
        assert_eq!(mesh.vertices.len(), 1 + 4 * 7);
        assert_eq!(mesh.indices.len(), 3 * 4 * 7);
        for vertex in &mesh.vertices {
            assert!(rect.expand(0.01).contains(vertex.pos));
        }
        let highest = mesh
            .vertices
            .iter()
            .min_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
            .unwrap();
        let lowest = mesh
            .vertices
            .iter()
            .max_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
            .unwrap();
        assert_eq!(highest.color, top);
        assert_eq!(lowest.color, bottom);
        assert_ne!(tile_colours(Icon::Globe), tile_colours(Icon::FolderOpen));
    }
}
