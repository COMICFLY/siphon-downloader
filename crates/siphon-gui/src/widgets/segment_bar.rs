use crate::theme::SiphonPalette;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, mouse};
use siphon_core::{SegmentInfo, SegmentState};

/// Continuous byte-map coverage bar (zero overlap even with 20+ dynamically stolen segments).
pub struct SegmentBar {
    pub segments: Vec<SegmentInfo>,
    pub total_size: Option<u64>,
    pub palette: SiphonPalette,
}

impl<Message> canvas::Program<Message> for SegmentBar {
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
        if w < 8.0 || h < 4.0 {
            return vec![frame.into_geometry()];
        }

        // Outer track
        let track = Path::rounded_rectangle(Point::ORIGIN, Size::new(w, h), 5.0.into());
        frame.fill(&track, self.palette.bg_input);

        if self.segments.is_empty() {
            return vec![frame.into_geometry()];
        }

        let pad = 2.0f32;
        let inner_h = (h - pad * 2.0).max(2.0);
        let usable_w = (w - pad * 2.0).max(1.0);
        let all_done = self.segments.iter().all(|s| s.state == SegmentState::Done);

        if all_done {
            let full = Path::rounded_rectangle(
                Point::new(pad, pad),
                Size::new(usable_w, inner_h),
                3.0.into(),
            );
            frame.fill(&full, self.palette.ok);
            return vec![frame.into_geometry()];
        }

        if let Some(total) = self.total_size
            && total > 0
        {
            // Sort by byte offset so boundaries render cleanly left-to-right
            let mut sorted: Vec<&SegmentInfo> = self.segments.iter().collect();
            sorted.sort_by_key(|s| s.start_offset);

            for seg in &sorted {
                let start_frac = (seg.start_offset as f64 / total as f64).clamp(0.0, 1.0);
                let end_byte = if seg.end_offset == u64::MAX {
                    total
                } else {
                    seg.end_offset.saturating_add(1).min(total)
                };
                let done_byte = if seg.state == SegmentState::Done {
                    end_byte
                } else {
                    seg.current_offset.clamp(seg.start_offset, end_byte)
                };
                if done_byte <= seg.start_offset {
                    continue;
                }

                let done_frac = (done_byte as f64 / total as f64).clamp(start_frac, 1.0);
                let x0 = pad + (start_frac as f32) * usable_w;
                let mut x1 = pad + (done_frac as f32) * usable_w;
                // Slightly close subpixel anti-aliasing seams when this segment touches the next
                if seg.state == SegmentState::Done && x1 < pad + usable_w {
                    x1 = (x1 + 0.5).min(pad + usable_w);
                }
                let fill_w = (x1 - x0).max(0.0);
                if fill_w < 0.25 {
                    continue;
                }

                let color = match seg.state {
                    SegmentState::Active => self.palette.accent,
                    SegmentState::Done => self.palette.ok,
                    SegmentState::Retrying => self.palette.warn,
                    SegmentState::Error => self.palette.err,
                    SegmentState::Pending => self.palette.text_muted,
                };
                let fill_rect = Path::rectangle(Point::new(x0, pad), Size::new(fill_w, inner_h));
                frame.fill(&fill_rect, color);

                if seg.state == SegmentState::Active && fill_w >= 2.0 {
                    let edge = Path::rectangle(
                        Point::new((x1 - 1.5).max(x0), pad),
                        Size::new(1.5, inner_h),
                    );
                    frame.fill(&edge, self.palette.accent_bright);
                }
            }

            // Only draw subtle boundary ticks between segments that haven't merged/completed yet,
            // so completed neighboring segments weld together seamlessly with zero dark gaps.
            let sep_color = Color {
                a: 0.45,
                ..self.palette.bg_input
            };
            let mut last_sep_x = -10.0f32;
            for pair in sorted.windows(2) {
                let prev = pair[0];
                let next = pair[1];
                let prev_reached_next =
                    prev.state == SegmentState::Done || prev.current_offset >= next.start_offset;
                if prev_reached_next {
                    continue;
                }
                let start_frac = (next.start_offset as f64 / total as f64).clamp(0.0, 1.0) as f32;
                let sep_x = pad + (start_frac * usable_w).round();
                if sep_x > pad + 1.0 && sep_x < w - pad - 1.0 && (sep_x - last_sep_x) >= 4.0 {
                    let sep = Path::rectangle(Point::new(sep_x, pad), Size::new(1.0, inner_h));
                    frame.fill(&sep, sep_color);
                    last_sep_x = sep_x;
                }
            }
        } else {
            // Single unknown-size stream fallback
            let fill = Path::rounded_rectangle(
                Point::new(pad, pad),
                Size::new(usable_w * 0.4, inner_h),
                3.0.into(),
            );
            frame.fill(&fill, self.palette.accent);
        }

        vec![frame.into_geometry()]
    }
}
