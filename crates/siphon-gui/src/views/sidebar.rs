use crate::i18n::Strings;
use crate::icons::{IconKind, icon};
use crate::state::{Message, NavSection};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_MONO_BOLD, FONT_REGULAR, SiphonPalette, clean_scrollable_style,
    hairline_rule_style, nav_btn_style, panel_style,
};
use iced::widget::{button, column, container, row, rule, scrollable, space, text};
use iced::{Alignment, Color, Element, Length};
use siphon_core::{DownloadTask, FileCategory, TaskStatus, format_speed};

pub fn view<'a>(
    active_nav: NavSection,
    tasks: &'a [DownloadTask],
    global_speed_bps: u64,
    active_count: usize,
    palette: SiphonPalette,
    strings: Strings,
) -> Element<'a, Message> {
    let total_count = tasks.len();
    let downloading_count = tasks.iter().filter(|t| t.status.is_active()).count();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let paused_count = tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Paused
                    | TaskStatus::Queued
                    | TaskStatus::Error
                    | TaskStatus::LinkExpired
            )
        })
        .count();

    let nav_item = |section: NavSection, label: &'static str, kind: IconKind, badge: Option<usize>| {
        let selected = active_nav == section;

        // Siphon signature: crisp 3px vertical Cool-Cyan tick on active row, transparent otherwise
        let indicator = container(space())
            .width(Length::Fixed(3.0))
            .height(Length::Fixed(16.0))
            .style(panel_style(
                if selected {
                    palette.accent
                } else {
                    Color::TRANSPARENT
                },
                None,
                2.0,
            ));

        let icon_box = container(icon(
            kind,
            15.0,
            if selected {
                palette.text_primary
            } else {
                palette.text_muted
            },
        ))
        .width(Length::Fixed(18.0))
        .center_x(Length::Fixed(18.0))
        .center_y(Length::Fill);

        let mut content = row![
            indicator,
            space::horizontal().width(6),
            icon_box,
            space::horizontal().width(10),
            text(label)
                .size(14)
                .font(if selected { FONT_MEDIUM } else { FONT_REGULAR })
                .color(if selected {
                    palette.text_primary
                } else {
                    palette.text_secondary
                }),
            space::horizontal(),
        ]
        .align_y(Alignment::Center)
        .height(Length::Fill);

        // Quiet monospace count (no glowing pill)
        if let Some(n) = badge
            && n > 0
        {
            content = content.push(
                text(n.to_string())
                    .size(12)
                    .font(FONT_MONO)
                    .color(if selected {
                        palette.text_secondary
                    } else {
                        palette.text_muted
                    }),
            );
        }

        button(content)
            .width(Length::Fill)
            .height(Length::Fixed(32.0))
            .padding(iced::Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 4.0,
            })
            .style(nav_btn_style(palette, selected))
            .on_press(Message::SelectNav(section))
    };

    let section_label = |title: &'static str| {
        container(
            text(title)
                .size(12)
                .font(FONT_MEDIUM)
                .color(palette.text_muted),
        )
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 3.0,
            left: 12.0,
        })
    };

    let categories = [
        FileCategory::Video,
        FileCategory::Archive,
        FileCategory::Software,
        FileCategory::Document,
        FileCategory::DiskImage,
        FileCategory::Other,
    ];

    let mut cat_col = column![section_label(strings.nav_categories())].spacing(2);
    for cat in categories {
        let count = tasks.iter().filter(|t| t.category == cat).count();
        cat_col = cat_col.push(nav_item(
            NavSection::Category(cat),
            strings.category_label(cat),
            IconKind::from_category(cat),
            if count > 0 { Some(count) } else { None },
        ));
    }

    let nav_list = column![
        section_label(strings.nav_views()),
        nav_item(
            NavSection::All,
            strings.all_tasks(),
            IconKind::AllTasks,
            Some(total_count),
        ),
        nav_item(
            NavSection::Downloading,
            strings.active_tasks(),
            IconKind::Downloading,
            Some(downloading_count),
        ),
        nav_item(
            NavSection::Completed,
            strings.completed_tasks(),
            IconKind::Completed,
            Some(completed_count),
        ),
        nav_item(
            NavSection::Paused,
            strings.paused_tasks(),
            IconKind::Paused,
            Some(paused_count),
        ),
        space::vertical().height(14),
        cat_col,
        space::vertical().height(14),
        section_label(strings.nav_workspace()),
        nav_item(NavSection::Rules, strings.rules(), IconKind::Rules, None),
        nav_item(
            NavSection::Plugins,
            strings.plugins(),
            IconKind::Plugins,
            None,
        ),
        nav_item(
            NavSection::Settings,
            strings.settings(),
            IconKind::Settings,
            None,
        ),
        nav_item(NavSection::About, strings.about(), IconKind::Info, None),
    ]
    .spacing(2)
    .padding(iced::Padding {
        top: 12.0,
        right: 12.0,
        bottom: 12.0,
        left: 12.0,
    });

    // Bottom live throughput telemetry readout (calm graphite card + live dot)
    let status_dot = container(space())
        .width(Length::Fixed(7.0))
        .height(Length::Fixed(7.0))
        .style(panel_style(
            if active_count > 0 {
                palette.accent
            } else {
                palette.text_muted
            },
            None,
            4.0,
        ));

    let telemetry = container(
        column![
            row![
                status_dot,
                text(if active_count > 0 {
                    format!("{active_count} 个传输活跃")
                } else if strings.lang == siphon_core::Language::ZhCn {
                    "引擎就绪 · 待机".to_string()
                } else {
                    "Engine idle".to_string()
                })
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(format_speed(global_speed_bps))
                .size(16)
                .font(FONT_MONO_BOLD)
                .color(if active_count > 0 {
                    palette.accent
                } else {
                    palette.text_primary
                }),
        ]
        .spacing(6),
    )
    .width(Length::Fill)
    .padding([12, 14])
    .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0));

    let body = column![
        scrollable(nav_list)
            .height(Length::Fill)
            .style(clean_scrollable_style(palette)),
        rule::horizontal(1).style(hairline_rule_style(palette)),
        container(telemetry).padding(12),
    ]
    .height(Length::Fill);

    container(body)
        .width(Length::Fixed(240.0))
        .height(Length::Fill)
        .style(panel_style(palette.bg_chrome, None, 0.0))
        .into()
}
