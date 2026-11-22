use crate::theme::{FONT_MONO, SiphonPalette};
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Theme, mouse};
use siphon_core::{SpeedSample, format_speed};

/// Real-time 60-second throughput area chart on a quiet graphite grid.
pub struct SpeedChart {
    pub samples: Vec<SpeedSample>,
    pub palette: SiphonPalette,
}

impl<Message> canvas::Program<Message> for SpeedChart {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        if w < 10.0 || h < 10.0 {
            return vec![frame.into_geometry()];
        }

        let pad_top = 20.0;
        let pad_bottom = 6.0;
        let pad_x = 4.0;
        let chart_w = (w - pad_x * 2.0).max(1.0);
        let chart_h = (h - pad_top - pad_bottom).max(1.0);

        let grid_color = self.palette.hairline;
        for frac in [0.0f32, 0.5, 1.0] {
            let y = pad_top + chart_h * frac;
            let line = Path::line(Point::new(pad_x, y), Point::new(w - pad_x, y));
            frame.stroke(
                &line,
                Stroke::default().with_width(1.0).with_color(grid_color),
            );
        }

        let peak = self
            .samples
            .iter()
            .map(|s| s.bytes_per_sec)
            .max()
            .unwrap_or(0);

        let peak_str = if peak > 0 {
            format!("MAX {}", format_speed(peak))
        } else {
            "IDLE".to_string()
        };
        frame.fill_text(Text {
            content: peak_str,
            position: Point::new(pad_x, 2.0),
            color: self.palette.text_muted,
            size: iced::Pixels(11.0),
            font: FONT_MONO,
            ..Text::default()
        });

        if self.samples.len() < 2 || peak == 0 {
            let baseline_y = pad_top + chart_h;
            let flat = Path::line(
                Point::new(pad_x, baseline_y),
                Point::new(w - pad_x, baseline_y),
            );
            frame.stroke(
                &flat,
                Stroke::default()
                    .with_width(1.5)
                    .with_color(self.palette.hairline_strong),
            );
            return vec![frame.into_geometry()];
        }

        let max_y = (peak as f64 * 1.15).max(1024.0);
        let n = self.samples.len();
        let step_x = chart_w / (n - 1).max(1) as f32;

        let points: Vec<Point> = self
            .samples
            .iter()
            .enumerate()
            .map(|(i, sample)| {
                let x = pad_x + i as f32 * step_x;
                let ratio = (sample.bytes_per_sec as f64 / max_y).clamp(0.0, 1.0) as f32;
                let y = pad_top + chart_h * (1.0 - ratio);
                Point::new(x, y)
            })
            .collect();

        let baseline_y = pad_top + chart_h;
        let area = Path::new(|b| {
            b.move_to(Point::new(points[0].x, baseline_y));
            for pt in &points {
                b.line_to(*pt);
            }
            if let Some(last) = points.last() {
                b.line_to(Point::new(last.x, baseline_y));
            }
            b.close();
        });
        frame.fill(
            &area,
            Color {
                a: if self.palette.is_dark { 0.16 } else { 0.12 },
                ..self.palette.accent
            },
        );

        let line = Path::new(|b| {
            b.move_to(points[0]);
            for pt in &points[1..] {
                b.line_to(*pt);
            }
        });
        frame.stroke(
            &line,
            Stroke::default()
                .with_width(1.8)
                .with_color(self.palette.accent),
        );

        if let Some(last) = points.last() {
            let dot = Path::circle(*last, 2.5);
            frame.fill(&dot, self.palette.accent_bright);
        }

        vec![frame.into_geometry()]
    }
}
