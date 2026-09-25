use egui::{pos2, Color32, Painter, Pos2, Rect, Stroke, Ui};

/// The Lucide icons the handout uses, drawn as strokes so no image loader is
/// needed. Coordinates are Lucide's own 24-unit grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Search,
    ChevronDown,
    ChevronRight,
    Refresh,
    ExternalLink,
    FolderOpen,
    Calculator,
    Globe,
    Terminal,
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
    let scale = rect.width().min(rect.height()) / 24.0;
    let stroke = Stroke::new(2.0 * scale, colour);
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
