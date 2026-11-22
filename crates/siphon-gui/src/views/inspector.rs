use crate::i18n::Strings;
use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_MONO_BOLD, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette,
    clean_scrollable_style, danger_btn_style, hairline_rule_style, input_style, outline_btn_style,
    panel_style, primary_btn_style, progress_style, subtle_btn_style,
};
use crate::widgets::segment_bar::SegmentBar;
use crate::widgets::speed_chart::SpeedChart;
use iced::widget::{
    button, canvas, column, container, image, mouse_area, progress_bar, row, rule, scrollable,
    space, text, text_input,
};
use iced::{Alignment, Element, Length};
use siphon_core::{
    DownloadTask, SegmentInfo, SegmentState, TaskStatus, format_bytes, format_eta, format_speed,
};

pub fn replace_url_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-replace-url")
}

pub fn view<'a>(
    task: &'a DownloadTask,
    file_icon: Option<&'a image::Handle>,
    show_replace_url: bool,
    replace_url_input: &'a str,
    palette: SiphonPalette,
    strings: Strings,
) -> Element<'a, Message> {
    let header = row![
        text(strings.inspector_title())
            .size(13)
            .font(FONT_MEDIUM)
            .color(palette.text_muted),
        space::horizontal(),
        button(
            row![
                icon(IconKind::Maximize, 11.0, palette.text_secondary),
                text("独立小窗").size(12).font(FONT_REGULAR),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([3, 8])
        .style(outline_btn_style(palette))
        .on_press(Message::OpenTaskDetailPopup(task.id)),
        button(icon(IconKind::Close, 14.0, palette.text_secondary))
            .padding([4, 6])
            .style(subtle_btn_style(palette, false))
            .on_press(Message::CloseInspector),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let status_color = palette.status_color(task.status);
    let title_icon_inner: Element<'a, Message> = if let Some(handle) = file_icon {
        image(handle.clone())
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .filter_method(image::FilterMethod::Linear)
            .into()
    } else {
        icon(
            IconKind::from_category(task.category),
            16.0,
            palette.text_secondary,
        )
        .into()
    };
    let title_icon = container(title_icon_inner)
        .center_x(Length::Fixed(36.0))
        .center_y(Length::Fixed(36.0))
        .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0));

    let title_block = row![
        title_icon,
        column![
            text(&task.filename)
                .size(15)
                .font(FONT_SEMIBOLD)
                .wrapping(text::Wrapping::WordOrGlyph)
                .color(palette.text_primary),
            row![
                container(space())
                    .width(Length::Fixed(6.0))
                    .height(Length::Fixed(6.0))
                    .style(panel_style(status_color, None, 3.0)),
                text(strings.status_label(task.status))
                    .size(12)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_secondary),
                text("·")
                    .size(12)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
                container(
                    text(task.domain())
                        .size(12)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_muted),
                )
                .width(Length::Fill)
                .clip(true),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        ]
        .spacing(4)
        .width(Length::Fill),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    // Visual Moment 1: Live Throughput Chart (60s)
    let speed_chart_card = container(
        column![
            row![
                text(strings.speed_chart_title())
                    .size(12)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                space::horizontal(),
                text(format_speed(task.speed_bps))
                    .size(14)
                    .font(FONT_MONO_BOLD)
                    .color(if task.speed_bps > 0 {
                        palette.accent
                    } else {
                        palette.text_primary
                    }),
            ]
            .align_y(Alignment::Center),
            canvas(SpeedChart {
                samples: task.speed_history.clone(),
                palette,
            })
            .width(Length::Fill)
            .height(Length::Fixed(112.0)),
            row![
                stat_cell("已下载", format_bytes(task.downloaded_bytes), palette),
                stat_cell(
                    "总大小",
                    task.total_size
                        .map(format_bytes)
                        .unwrap_or_else(|| "未知".into()),
                    palette
                ),
                stat_cell("预计剩余", format_eta(task.eta_secs), palette),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(14)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Visual Moment 2: Continuous Byte-Map Strip + Crisp 2-Column Per-Segment Grid
    let active_segs = task
        .segments
        .iter()
        .filter(|s| s.state == SegmentState::Active)
        .count();
    let done_segs = task
        .segments
        .iter()
        .filter(|s| s.state == SegmentState::Done)
        .count();

    let mut seg_col = column![
        row![
            text(strings.segments_title())
                .size(12)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
            space::horizontal(),
            text(format!(
                "{active_segs} 活跃 · {done_segs}/{}",
                task.segments.len()
            ))
            .size(12)
            .font(FONT_MONO)
            .color(palette.text_muted),
        ]
        .align_y(Alignment::Center),
        canvas(SegmentBar {
            segments: task.segments.clone(),
            total_size: task.total_size,
            palette,
        })
        .width(Length::Fill)
        .height(Length::Fixed(14.0)),
    ]
    .spacing(10);

    if !task.segments.is_empty() {
        let mut sorted: Vec<&SegmentInfo> = task.segments.iter().collect();
        sorted.sort_by_key(|s| s.start_offset);

        let mut grid = column![].spacing(6);
        for pair in sorted.chunks(2).take(8) {
            let mut r = row![segment_mini_cell(pair[0], palette)].spacing(6);
            if pair.len() > 1 {
                r = r.push(segment_mini_cell(pair[1], palette));
            } else {
                r = r.push(container(space()).width(Length::Fill));
            }
            grid = grid.push(r);
        }
        if sorted.len() > 16 {
            grid = grid.push(
                text(format!("其余 {} 个已完成/借片分段已折叠", sorted.len() - 16))
                    .size(11)
                    .font(FONT_MONO)
                    .color(palette.text_muted),
            );
        }
        seg_col = seg_col.push(grid);
    }

    seg_col = seg_col.push(
        row![
            legend_dot(palette.accent, "下载中", palette),
            legend_dot(palette.ok, "完成", palette),
            legend_dot(palette.warn, "重试", palette),
            legend_dot(palette.text_muted, "等待", palette),
        ]
        .spacing(12),
    );

    let segment_card = container(seg_col)
        .padding(14)
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    let limit_btn = |label: &'static str, bps: u64| {
        let active = task.speed_limit_bps == bps;
        button(text(label).size(12).font(FONT_MONO))
            .padding([4, 9])
            .style(subtle_btn_style(palette, active))
            .on_press(Message::SetTaskSpeedLimit(task.id, bps))
    };

    let controls_card = container(
        column![
            row![
                text(strings.connections())
                    .size(13)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary),
                space::horizontal(),
                button(text("-").size(13).font(FONT_MONO))
                    .padding([3, 10])
                    .style(outline_btn_style(palette))
                    .on_press(Message::AdjustTaskConnections(task.id, -1)),
                text(format!("{}", task.max_connections))
                    .size(13)
                    .font(FONT_MONO_BOLD)
                    .color(palette.text_primary),
                button(text("+").size(13).font(FONT_MONO))
                    .padding([3, 10])
                    .style(outline_btn_style(palette))
                    .on_press(Message::AdjustTaskConnections(task.id, 1)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            column![
                row![
                    text(strings.speed_limit())
                        .size(13)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                    space::horizontal(),
                    text(if task.speed_limit_bps == 0 {
                        strings.unlimited().to_string()
                    } else {
                        format_speed(task.speed_limit_bps)
                    })
                    .size(12)
                    .font(FONT_MONO)
                    .color(palette.text_primary),
                ],
                row![
                    limit_btn("不限", 0),
                    limit_btn("1M", 1024 * 1024),
                    limit_btn("5M", 5 * 1024 * 1024),
                    limit_btn("20M", 20 * 1024 * 1024),
                ]
                .spacing(4),
            ]
            .spacing(8),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            column![
                text("保存目录")
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_muted),
                text(task.save_dir.to_string_lossy().to_string())
                    .size(12)
                    .font(FONT_MONO)
                    .wrapping(text::Wrapping::WordOrGlyph)
                    .color(palette.text_secondary),
            ]
            .spacing(3),
        ]
        .spacing(12),
    )
    .padding(14)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    let mut extra_section = column![].spacing(8);
    if let Some(err) = &task.error_message {
        extra_section = extra_section.push(
            container(
                text(err)
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(if task.status == TaskStatus::LinkExpired {
                        palette.warn
                    } else {
                        palette.err
                    }),
            )
            .padding(12)
            .width(Length::Fill)
            .style(panel_style(palette.status_soft_bg(task.status), None, 8.0)),
        );
    }

    if show_replace_url || task.status == TaskStatus::LinkExpired {
        let replace_field = mouse_area(
            text_input("https://...", replace_url_input)
                .id(replace_url_id())
                .on_input(Message::ReplaceUrlInputChanged)
                .on_submit(Message::SubmitReplaceUrl(task.id))
                .font(FONT_REGULAR)
                .size(13)
                .padding([8, 10])
                .style(input_style(palette)),
        )
        .on_release(Message::FocusInputTarget(InputTarget::ReplaceUrl))
        .on_right_press(Message::OpenInputContextMenu(InputTarget::ReplaceUrl));

        extra_section = extra_section.push(
            container(
                column![
                    text("替换新直链（保留断点进度）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    replace_field,
                    button(
                        container(text("替换并继续").size(13).font(FONT_MEDIUM))
                            .width(Length::Fill)
                            .center_x(Length::Fill),
                    )
                    .padding([7, 12])
                    .width(Length::Fill)
                    .style(primary_btn_style(palette))
                    .on_press(Message::SubmitReplaceUrl(task.id)),
                ]
                .spacing(8),
            )
            .padding(12)
            .width(Length::Fill)
            .style(panel_style(
                palette.bg_card,
                Some(palette.hairline_strong),
                10.0,
            )),
        );
    }

    let primary_op = if task.status.is_active() || task.status == TaskStatus::Queued {
        button(
            container(
                row![
                    icon(IconKind::Pause, 13.0, palette.cta_text),
                    text("暂停").size(13).font(FONT_MEDIUM),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .height(Length::Fixed(34.0))
        .padding([0, 12])
        .width(Length::Fill)
        .style(primary_btn_style(palette))
        .on_press(Message::PauseTask(task.id))
    } else if task.status == TaskStatus::Completed {
        button(
            container(
                row![
                    icon(IconKind::Play, 13.0, palette.cta_text),
                    text(strings.open_file()).size(13).font(FONT_MEDIUM),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .height(Length::Fixed(34.0))
        .padding([0, 12])
        .width(Length::Fill)
        .style(primary_btn_style(palette))
        .on_press(Message::OpenTaskFile(task.id))
    } else {
        button(
            container(
                row![
                    icon(IconKind::Play, 13.0, palette.cta_text),
                    text("继续下载").size(13).font(FONT_MEDIUM),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .height(Length::Fixed(34.0))
        .padding([0, 12])
        .width(Length::Fill)
        .style(primary_btn_style(palette))
        .on_press(Message::ResumeTask(task.id))
    };

    let actions_grid = column![
        row![
            primary_op,
            button(
                container(
                    row![
                        icon(IconKind::Folder, 14.0, palette.text_secondary),
                        text(strings.open_folder()).size(13).font(FONT_REGULAR),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(34.0))
            .padding([0, 12])
            .style(outline_btn_style(palette))
            .on_press(Message::RevealTaskInExplorer(task.id)),
        ]
        .spacing(8),
        row![
            button(
                container(
                    row![
                        icon(IconKind::Link, 13.0, palette.text_secondary),
                        text(strings.replace_link()).size(12).font(FONT_REGULAR),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
            )
            .height(Length::Fixed(32.0))
            .padding([0, 10])
            .width(Length::Fill)
            .style(outline_btn_style(palette))
            .on_press(Message::ToggleReplaceUrlBox),
            button(
                container(
                    row![
                        icon(IconKind::Trash, 13.0, palette.err),
                        text(strings.delete_task()).size(12).font(FONT_REGULAR),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(32.0))
            .padding([0, 10])
            .style(danger_btn_style(palette))
            .on_press(Message::RequestDeleteTask(task.id)),
        ]
        .spacing(8),
    ]
    .spacing(8);

    let mut logs_col = column![
        text(strings.logs_title())
            .size(12)
            .font(FONT_MEDIUM)
            .color(palette.text_secondary)
    ]
    .spacing(5);

    for entry in task.logs.iter().rev().take(12) {
        let lvl_color = match entry.level.as_str() {
            "ERROR" => palette.err,
            "WARN" => palette.warn,
            _ => palette.text_muted,
        };
        logs_col = logs_col.push(
            text(format!(
                "[{}] {}",
                entry.timestamp.format("%H:%M:%S"),
                entry.message
            ))
            .size(11)
            .font(FONT_MONO)
            .color(lvl_color),
        );
    }

    let logs_card = container(logs_col)
        .padding(12)
        .width(Length::Fill)
        .style(panel_style(palette.bg_input, Some(palette.hairline), 10.0));

    let body = column![
        header,
        title_block,
        speed_chart_card,
        segment_card,
        extra_section,
        controls_card,
        actions_grid,
        logs_card,
    ]
    .spacing(12)
    .padding(16);

    container(
        scrollable(body)
            .height(Length::Fill)
            .style(clean_scrollable_style(palette)),
    )
    .width(Length::Fixed(396.0))
    .height(Length::Fill)
    .style(panel_style(palette.bg_inspector, None, 0.0))
    .into()
}

fn segment_mini_cell<'a>(seg: &SegmentInfo, palette: SiphonPalette) -> Element<'a, Message> {
    let color = match seg.state {
        SegmentState::Active => palette.accent,
        SegmentState::Done => palette.ok,
        SegmentState::Retrying => palette.warn,
        SegmentState::Error => palette.err,
        SegmentState::Pending => palette.text_muted,
    };
    let ratio = seg.progress_ratio();
    let right_label = match seg.state {
        SegmentState::Done => "完成".to_string(),
        SegmentState::Active if seg.speed_bps > 0 => format_speed(seg.speed_bps),
        SegmentState::Retrying => "重试".to_string(),
        SegmentState::Error => "失败".to_string(),
        _ => format!("{:.0}%", ratio * 100.0),
    };

    container(
        column![
            row![
                text(format!("#{:02}", seg.index + 1))
                    .size(11)
                    .font(FONT_MONO)
                    .color(palette.text_muted),
                space::horizontal(),
                text(right_label)
                    .size(11)
                    .font(FONT_MONO)
                    .color(if seg.state == SegmentState::Active {
                        palette.accent
                    } else if seg.state == SegmentState::Done {
                        palette.ok
                    } else {
                        palette.text_secondary
                    }),
            ]
            .align_y(Alignment::Center),
            progress_bar(0.0..=1.0, ratio)
                .girth(Length::Fixed(3.0))
                .style(progress_style(palette, color)),
        ]
        .spacing(5),
    )
    .width(Length::Fill)
    .padding([6, 8])
    .style(panel_style(palette.bg_input, Some(palette.hairline), 6.0))
    .into()
}

fn stat_cell<'a>(
    label: &'static str,
    value: String,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    container(
        column![
            text(label)
                .size(11)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
            text(value)
                .size(13)
                .font(FONT_MONO)
                .color(palette.text_primary),
        ]
        .spacing(2),
    )
    .width(Length::Fill)
    .padding([6, 8])
    .style(panel_style(palette.bg_input, None, 6.0))
    .into()
}

fn legend_dot<'a>(
    color: iced::Color,
    label: &'static str,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    row![
        container(space())
            .width(Length::Fixed(6.0))
            .height(Length::Fixed(6.0))
            .style(panel_style(color, None, 3.0)),
        text(label)
            .size(12)
            .font(FONT_REGULAR)
            .color(palette.text_muted),
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .into()
}
