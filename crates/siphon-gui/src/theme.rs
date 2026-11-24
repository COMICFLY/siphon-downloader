use iced::font::{Family, Stretch, Style, Weight};
use iced::widget::{button, checkbox, container, progress_bar, rule, scrollable, text_input};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector};
use siphon_core::{TaskStatus, ThemeMode};

pub const FONT_REGULAR: Font = Font {
    family: Family::Name("MiSans"),
    weight: Weight::Normal,
    stretch: Stretch::Normal,
    style: Style::Normal,
};

pub const FONT_MEDIUM: Font = Font {
    family: Family::Name("MiSans"),
    weight: Weight::Medium,
    stretch: Stretch::Normal,
    style: Style::Normal,
};

pub const FONT_SEMIBOLD: Font = Font {
    family: Family::Name("MiSans"),
    weight: Weight::Semibold,
    stretch: Stretch::Normal,
    style: Style::Normal,
};

pub const FONT_MONO: Font = Font {
    family: Family::Name("Consolas"),
    weight: Weight::Normal,
    stretch: Stretch::Normal,
    style: Style::Normal,
};

pub const FONT_MONO_BOLD: Font = Font {
    family: Family::Name("Consolas"),
    weight: Weight::Bold,
    stretch: Stretch::Normal,
    style: Style::Normal,
};

/// Calm Graphite neutral ramp with a single disciplined Cool-Cyan accent reserved for live data.
#[derive(Debug, Clone, Copy)]
pub struct SiphonPalette {
    pub is_dark: bool,

    pub bg_window: Color,
    pub bg_chrome: Color,
    pub bg_main: Color,
    pub bg_inspector: Color,
    pub bg_card: Color,
    pub bg_raised: Color,
    pub bg_input: Color,
    pub bg_modal: Color,
    pub bg_overlay: Color,

    pub hairline: Color,
    pub hairline_strong: Color,

    /// Quiet high-craft primary CTA fill (off-white on dark, near-black on light).
    pub cta_bg: Color,
    pub cta_bg_hover: Color,
    pub cta_text: Color,

    /// Single Cool-Cyan accent reserved for active progress, live speed curve, segments & 3px ticks.
    pub accent: Color,
    pub accent_bright: Color,
    pub accent_soft: Color,

    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,

    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub ok_soft: Color,
    pub warn_soft: Color,
    pub err_soft: Color,
}

impl SiphonPalette {
    pub fn resolve(mode: ThemeMode, system_dark: bool, use_mica: bool) -> Self {
        let is_dark = match mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => system_dark,
        };
        if is_dark {
            Self::dark(use_mica)
        } else {
            Self::light(use_mica)
        }
    }

    pub fn dark(use_mica: bool) -> Self {
        let a = |mica_alpha: f32| if use_mica { mica_alpha } else { 1.0 };

        Self {
            is_dark: true,
            bg_window: if use_mica {
                Color::TRANSPARENT
            } else {
                Color::from_rgb8(0x0F, 0x10, 0x12)
            },
            bg_chrome: Color::from_rgba8(0x0F, 0x10, 0x12, a(0.84)),
            bg_main: Color::from_rgba8(0x14, 0x15, 0x18, a(0.92)),
            bg_inspector: Color::from_rgba8(0x11, 0x12, 0x15, a(0.94)),
            bg_card: Color::from_rgba8(0x19, 0x1B, 0x1F, a(0.92)),
            bg_raised: Color::from_rgba8(0x22, 0x24, 0x2A, a(0.96)),
            bg_input: Color::from_rgba8(0x16, 0x17, 0x1B, a(0.94)),
            bg_modal: Color::from_rgb8(0x16, 0x18, 0x1C),
            bg_overlay: Color::from_rgba8(0x05, 0x06, 0x08, 0.68),

            hairline: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.025),
            hairline_strong: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.065),

            cta_bg: Color::from_rgb8(0xEC, 0xED, 0xF0),
            cta_bg_hover: Color::from_rgb8(0xFF, 0xFF, 0xFF),
            cta_text: Color::from_rgb8(0x0F, 0x10, 0x12),

            accent: Color::from_rgb8(0x38, 0xBD, 0xF8),
            accent_bright: Color::from_rgb8(0x5E, 0xD3, 0xFA),
            accent_soft: Color::from_rgba8(0x38, 0xBD, 0xF8, 0.08),

            text_primary: Color::from_rgb8(0xED, 0xED, 0xEF),
            text_secondary: Color::from_rgb8(0x9A, 0x9C, 0xA6),
            text_muted: Color::from_rgb8(0x6B, 0x6E, 0x78),

            ok: Color::from_rgb8(0x4C, 0xC3, 0x8A),
            warn: Color::from_rgb8(0xE5, 0xA9, 0x3B),
            err: Color::from_rgb8(0xE5, 0x53, 0x53),
            ok_soft: Color::from_rgba8(0x4C, 0xC3, 0x8A, 0.08),
            warn_soft: Color::from_rgba8(0xE5, 0xA9, 0x3B, 0.08),
            err_soft: Color::from_rgba8(0xE5, 0x53, 0x53, 0.09),
        }
    }

    pub fn light(use_mica: bool) -> Self {
        let a = |mica_alpha: f32| if use_mica { mica_alpha } else { 1.0 };

        Self {
            is_dark: false,
            bg_window: if use_mica {
                Color::TRANSPARENT
            } else {
                Color::from_rgb8(0xF5, 0xF5, 0xF7)
            },
            bg_chrome: Color::from_rgba8(0xF4, 0xF5, 0xF7, a(0.84)),
            bg_main: Color::from_rgba8(0xFA, 0xFA, 0xFB, a(0.94)),
            bg_inspector: Color::from_rgba8(0xFF, 0xFF, 0xFF, a(0.96)),
            bg_card: Color::from_rgba8(0xFF, 0xFF, 0xFF, a(0.96)),
            bg_raised: Color::from_rgba8(0xEC, 0xED, 0xF1, a(0.98)),
            bg_input: Color::from_rgba8(0xFF, 0xFF, 0xFF, a(0.96)),
            bg_modal: Color::from_rgb8(0xFF, 0xFF, 0xFF),
            bg_overlay: Color::from_rgba8(0x0F, 0x10, 0x14, 0.52),

            hairline: Color::from_rgba8(0x00, 0x00, 0x00, 0.14),
            hairline_strong: Color::from_rgba8(0x00, 0x00, 0x00, 0.24),

            cta_bg: Color::from_rgb8(0x16, 0x17, 0x1A),
            cta_bg_hover: Color::from_rgb8(0x28, 0x2A, 0x30),
            cta_text: Color::WHITE,

            accent: Color::from_rgb8(0x02, 0x84, 0xC7),
            accent_bright: Color::from_rgb8(0x0E, 0xA5, 0xE9),
            accent_soft: Color::from_rgba8(0x02, 0x84, 0xC7, 0.16),

            text_primary: Color::from_rgb8(0x16, 0x17, 0x1A),
            text_secondary: Color::from_rgb8(0x56, 0x58, 0x62),
            text_muted: Color::from_rgb8(0x7C, 0x7F, 0x8A),

            ok: Color::from_rgb8(0x16, 0xA3, 0x4A),
            warn: Color::from_rgb8(0xD9, 0x77, 0x06),
            err: Color::from_rgb8(0xDC, 0x26, 0x26),
            ok_soft: Color::from_rgba8(0x16, 0xA3, 0x4A, 0.16),
            warn_soft: Color::from_rgba8(0xD9, 0x77, 0x06, 0.16),
            err_soft: Color::from_rgba8(0xDC, 0x26, 0x26, 0.16),
        }
    }

    pub fn iced_theme(&self) -> Theme {
        if self.is_dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    }

    pub fn status_color(&self, status: TaskStatus) -> Color {
        match status {
            TaskStatus::Downloading | TaskStatus::Probing | TaskStatus::Verifying => self.accent,
            TaskStatus::Completed | TaskStatus::Seeding => self.ok,
            TaskStatus::Queued | TaskStatus::Paused => self.text_secondary,
            TaskStatus::LinkExpired => self.warn,
            TaskStatus::Error => self.err,
        }
    }

    pub fn status_soft_bg(&self, status: TaskStatus) -> Color {
        match status {
            TaskStatus::Downloading | TaskStatus::Probing | TaskStatus::Verifying => self.accent_soft,
            TaskStatus::Completed | TaskStatus::Seeding => self.ok_soft,
            TaskStatus::Queued | TaskStatus::Paused => self.bg_raised,
            TaskStatus::LinkExpired => self.warn_soft,
            TaskStatus::Error => self.err_soft,
        }
    }

    /// Return a copy of this palette with a custom `#RRGGBB` accent color applied to live telemetry,
    /// active progress bars, speed chart, checkboxes, and highlight pills.
    pub fn with_custom_accent(mut self, hex: &str) -> Self {
        let Some((r, g, b)) = parse_hex_rgb(hex) else {
            return self;
        };
        if self.is_dark {
            self.accent = Color::from_rgb8(r, g, b);
            let br = ((r as u16 * 3 + 255) / 4).min(255) as u8;
            let bg = ((g as u16 * 3 + 255) / 4).min(255) as u8;
            let bb = ((b as u16 * 3 + 255) / 4).min(255) as u8;
            self.accent_bright = Color::from_rgb8(br, bg, bb);
            self.accent_soft = Color::from_rgba8(r, g, b, 0.13);
        } else {
            // Keep light-mode accent legible on white/alabaster surfaces
            let lum = 0.2126 * (r as f32 / 255.0)
                + 0.7152 * (g as f32 / 255.0)
                + 0.0722 * (b as f32 / 255.0);
            let factor = if lum > 0.62 { 0.76 } else { 0.90 };
            let dr = (r as f32 * factor).round().clamp(0.0, 255.0) as u8;
            let dg = (g as f32 * factor).round().clamp(0.0, 255.0) as u8;
            let db = (b as f32 * factor).round().clamp(0.0, 255.0) as u8;
            self.accent = Color::from_rgb8(dr, dg, db);
            self.accent_bright = Color::from_rgb8(r, g, b);
            self.accent_soft = Color::from_rgba8(dr, dg, db, 0.11);
        }
        self
    }
}

pub const ACCENT_PRESETS: &[(&str, &str)] = &[
    ("冷青蓝", "#38BDF8"),
    ("翠绿", "#22C55E"),
    ("靛蓝", "#6366F1"),
    ("琥珀橙", "#F59E0B"),
    ("珊瑚红", "#F43F5E"),
    ("极光紫", "#A855F7"),
    ("玫瑰粉", "#EC4899"),
    ("石墨银", "#94A3B8"),
];

pub fn parse_hex_rgb(raw: &str) -> Option<(u8, u8, u8)> {
    let s = raw.trim().trim_start_matches('#');
    if s.len() == 6 && s.is_ascii() {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some((r, g, b))
    } else {
        None
    }
}

pub fn normalize_hex_color(raw: &str) -> Option<String> {
    let (r, g, b) = parse_hex_rgb(raw)?;
    Some(format!("#{r:02X}{g:02X}{b:02X}"))
}

pub fn swatch_btn_style(
    swatch: Color,
    selected: bool,
    palette: SiphonPalette,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let border_col = if selected {
            palette.text_primary
        } else if matches!(status, button::Status::Hovered) {
            palette.text_secondary
        } else {
            Color::from_rgba8(255, 255, 255, if palette.is_dark { 0.15 } else { 0.25 })
        };
        button::Style {
            background: Some(Background::Color(swatch)),
            text_color: Color::WHITE,
            border: Border {
                color: border_col,
                width: if selected { 2.0 } else { 1.0 },
                radius: 12.0.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn panel_style(
    bg: Color,
    border: Option<Color>,
    radius: f32,
) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border.unwrap_or(Color::TRANSPARENT),
            width: if border.is_some() { 1.0 } else { 0.0 },
            radius: radius.into(),
        },
        ..Default::default()
    }
}

pub fn hairline_rule_style(p: SiphonPalette) -> impl Fn(&Theme) -> rule::Style {
    move |_| rule::Style {
        color: p.hairline,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

pub fn modal_card_style(p: SiphonPalette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.bg_modal)),
        border: Border {
            color: p.hairline_strong,
            width: 1.0,
            radius: 14.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, if p.is_dark { 0.56 } else { 0.16 }),
            offset: Vector::new(0.0, 16.0),
            blur_radius: 40.0,
        },
        ..Default::default()
    }
}

/// Crisp off-white / dark-ink primary CTA button (Linear / Raycast aesthetic).
pub fn primary_btn_style(p: SiphonPalette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => p.cta_bg_hover,
            button::Status::Pressed => p.cta_bg,
            button::Status::Disabled => p.bg_raised,
            _ => p.cta_bg,
        };
        let text_color = if matches!(status, button::Status::Disabled) {
            p.text_muted
        } else {
            p.cta_text
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 8.0.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Quiet navigation / ghost button — active uses neutral raised surface, zero neon glow.
pub fn nav_btn_style(
    p: SiphonPalette,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = if active {
            p.bg_raised
        } else {
            match status {
                button::Status::Hovered => Color {
                    a: if p.is_dark { 0.52 } else { 0.65 },
                    ..p.bg_raised
                },
                button::Status::Pressed => p.bg_raised,
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if active {
                p.text_primary
            } else {
                p.text_secondary
            },
            border: Border {
                color: if active {
                    p.hairline
                } else {
                    Color::TRANSPARENT
                },
                width: if active { 1.0 } else { 0.0 },
                radius: 8.0.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn subtle_btn_style(
    p: SiphonPalette,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    nav_btn_style(p, active)
}

pub fn outline_btn_style(p: SiphonPalette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => p.bg_raised,
            button::Status::Pressed => p.bg_raised,
            _ => p.bg_card,
        };
        let border_col = match status {
            button::Status::Hovered => p.hairline_strong,
            _ => p.hairline,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.text_primary,
            border: Border {
                color: border_col,
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn danger_btn_style(p: SiphonPalette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => p.err,
            _ => p.err_soft,
        };
        let text = match status {
            button::Status::Hovered | button::Status::Pressed => Color::WHITE,
            _ => p.err,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: text,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 8.0.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Solid high-emphasis danger button for destructive modal confirmations (e.g. deleting local files).
pub fn danger_solid_btn_style(p: SiphonPalette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => Color::from_rgb8(0xEF, 0x44, 0x44),
            button::Status::Pressed => Color::from_rgb8(0xC8, 0x35, 0x35),
            button::Status::Disabled => p.bg_raised,
            _ => p.err,
        };
        let text = if matches!(status, button::Status::Disabled) {
            p.text_muted
        } else {
            Color::WHITE
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: text,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 8.0.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn checkbox_style(
    p: SiphonPalette,
    danger: bool,
) -> impl Fn(&Theme, checkbox::Status) -> checkbox::Style {
    move |_, status| {
        let (checked, hovered) = match status {
            checkbox::Status::Active { is_checked } => (is_checked, false),
            checkbox::Status::Hovered { is_checked } => (is_checked, true),
            checkbox::Status::Disabled { is_checked } => (is_checked, false),
        };
        let active_fill = if danger { p.err } else { p.accent };
        checkbox::Style {
            background: Background::Color(if checked {
                active_fill
            } else if hovered {
                p.bg_raised
            } else {
                p.bg_input
            }),
            icon_color: if danger || !p.is_dark {
                Color::WHITE
            } else {
                Color::from_rgb8(0x0B, 0x10, 0x15)
            },
            border: Border {
                color: if checked {
                    active_fill
                } else if hovered {
                    p.text_muted
                } else {
                    p.hairline_strong
                },
                width: 1.0,
                radius: 5.0.into(),
            },
            text_color: Some(if danger && checked {
                p.text_primary
            } else {
                p.text_secondary
            }),
        }
    }
}

pub fn window_ctrl_btn_style(
    p: SiphonPalette,
    is_close: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed if is_close => {
                Color::from_rgb8(0xDC, 0x26, 0x26)
            }
            button::Status::Hovered => p.bg_raised,
            button::Status::Pressed => p.hairline_strong,
            _ => Color::TRANSPARENT,
        };
        let text = match status {
            button::Status::Hovered | button::Status::Pressed if is_close => Color::WHITE,
            button::Status::Hovered => p.text_primary,
            _ => p.text_secondary,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: text,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn task_row_btn_style(
    p: SiphonPalette,
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = if selected {
            p.bg_raised
        } else {
            match status {
                button::Status::Hovered => Color {
                    a: if p.is_dark { 0.72 } else { 1.0 },
                    ..p.bg_raised
                },
                button::Status::Pressed => p.bg_raised,
                _ => p.bg_card,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.text_primary,
            border: Border {
                color: if selected {
                    p.hairline_strong
                } else {
                    p.hairline
                },
                width: 1.0,
                radius: 10.0.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn input_style(p: SiphonPalette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let border_color = match status {
            text_input::Status::Focused { .. } => p.hairline_strong,
            text_input::Status::Hovered => p.hairline_strong,
            _ => p.hairline,
        };
        text_input::Style {
            background: Background::Color(p.bg_input),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 8.0.into(),
            },
            icon: p.text_muted,
            placeholder: p.text_muted,
            value: p.text_primary,
            selection: p.accent_soft,
        }
    }
}

/// Borderless embedded input used inside a composite search shell (with keycap on the right).
pub fn seamless_input_style(
    p: SiphonPalette,
) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, _| text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: p.text_muted,
        placeholder: p.text_muted,
        value: p.text_primary,
        selection: p.accent_soft,
    }
}

pub fn progress_style(
    p: SiphonPalette,
    fill_color: Color,
) -> impl Fn(&Theme) -> progress_bar::Style {
    move |_| progress_bar::Style {
        background: Background::Color(p.hairline),
        bar: Background::Color(fill_color),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 2.0.into(),
        },
    }
}

pub fn clean_scrollable_style(
    p: SiphonPalette,
) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut base = scrollable::default(theme, status);
        base.vertical_rail.background = None;
        base.vertical_rail.border = Border::default();
        base.vertical_rail.scroller.background = Background::Color(p.hairline_strong);
        base.vertical_rail.scroller.border = Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        };
        base
    }
}
