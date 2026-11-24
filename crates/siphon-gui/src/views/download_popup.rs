use crate::icons::{IconKind, icon};
use crate::state::{DownloadPopupState, Message};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_MONO_BOLD, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette,
    checkbox_style, danger_btn_style, hairline_rule_style, outline_btn_style, panel_style,
    primary_btn_style, progress_style, window_ctrl_btn_style,
};
use crate::widgets::segment_bar::SegmentBar;
use crate::widgets::speed_chart::SpeedChart;
use iced::widget::{
    button, canvas, checkbox, column, container, image, mouse_area, progress_bar, row, rule,
    space, text,
};
use iced::{Alignment, Color, Element, Length};
use siphon_core::{
    DownloadTask, SegmentState, TaskStatus, format_bytes, format_eta, format_speed,
};

pub fn view<'a>(
    popup: &'a DownloadPopupState,
    task: Option<&'a DownloadTask>,
    sys_icon: Option<&'a image::Handle>,
    download_popup_accent: &str,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    // Only the download telemetry (progress bar, speed value, speed curve, segment bar) uses the popup accent color (default Green #22C55E)
    let green_palette = palette.with_custom_accent(download_popup_accent);
    let dl_green = green_palette.accent;
    let dl_green_soft = green_palette.accent_soft;
    let win_id = popup.window_id;

    let Some(task) = task else {
        return container(
            column![
                text("任务已结束或已被移除")
                    .size(14)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                button(text("关闭窗口").size(13).font(FONT_REGULAR))
                    .padding([7, 16])
                    .style(outline_btn_style(palette))
                    .on_press(Message::DownloadPopupClose(win_id)),
            ]
            .spacing(12)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(panel_style(
            palette.bg_main,
            Some(palette.hairline_strong),
            0.0,
        ))
        .into();
    };

    let ratio = task.progress_ratio();
    let pct_str = if task.total_size.is_some() || task.status.is_finished() {
        format!("{:.1}%", ratio * 100.0)
    } else {
        "—".to_string()
    };

    let (status_badge_text, status_fg, status_bg) = match task.status {
        TaskStatus::Downloading => (format!("正在下载 · {pct_str}"), dl_green, dl_green_soft),
        TaskStatus::Probing => ("正在连接源站...".to_string(), dl_green, dl_green_soft),
        TaskStatus::Verifying => ("正在校验文件...".to_string(), dl_green, dl_green_soft),
        TaskStatus::Seeding => ("做种中 · 100%".to_string(), palette.ok, palette.ok_soft),
        TaskStatus::Completed => ("下载完成 · 100%".to_string(), palette.ok, palette.ok_soft),
        TaskStatus::Paused => (format!("已暂停 · {pct_str}"), palette.warn, palette.warn_soft),
        TaskStatus::Queued => ("排队等待中".to_string(), palette.text_secondary, palette.bg_input),
        TaskStatus::LinkExpired => ("直链已过期".to_string(), palette.warn, palette.warn_soft),
        TaskStatus::Error => ("传输错误".to_string(), palette.err, palette.err_soft),
    };

    let drag_brand = mouse_area(
        container(
            row![
                icon(IconKind::Brand, 17.0, palette.accent),
                text("Siphon")
                    .size(14)
                    .font(FONT_SEMIBOLD)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_primary),
                container(
                    text(status_badge_text)
                        .size(11)
                        .font(FONT_MONO)
                        .wrapping(text::Wrapping::None)
                        .color(status_fg),
                )
                .padding([2, 8])
                .style(panel_style(status_bg, None, 5.0)),
                container(
                    text(&task.filename)
                        .size(12)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_muted),
                )
                .width(Length::Fill)
                .clip(true),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([0, 16])
        .align_y(Alignment::Center),
    )
    .on_press(Message::DownloadPopupDrag(win_id));

    let close_btn = button(
        container(icon(IconKind::Close, 13.0, palette.text_secondary))
            .center_x(Length::Fixed(36.0))
            .center_y(Length::Fixed(30.0)),
    )
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(30.0))
    .padding(0)
    .style(window_ctrl_btn_style(palette, true))
    .on_press(Message::DownloadPopupClose(win_id));

    let top_bar = container(
        row![
            drag_brand,
            container(close_btn).padding([0, 8]).align_y(Alignment::Center),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(42.0))
        .align_y(Alignment::Center),
    )
    .style(panel_style(palette.bg_chrome, None, 0.0));

    let icon_inner: Element<'a, Message> = if let Some(handle) = sys_icon {
        image(handle.clone())
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(image::FilterMethod::Linear)
            .into()
    } else {
        icon(
            IconKind::from_category(task.category),
            20.0,
            palette.text_secondary,
        )
        .into()
    };

    let file_icon_box = container(icon_inner)
        .center_x(Length::Fixed(42.0))
        .center_y(Length::Fixed(42.0))
        .style(panel_style(palette.bg_input, Some(palette.hairline), 10.0));

    let resume_pill = if task.supports_range {
        pill("支持断点续传".to_string(), palette.ok, palette.ok_soft, palette)
    } else {
        pill("单线程流".to_string(), palette.warn, palette.warn_soft, palette)
    };

    let threads_pill = pill(
        format!("{} 线程", task.max_connections),
        palette.text_secondary,
        palette.bg_input,
        palette,
    );

    let file_header = row![
        file_icon_box,
        column![
            row![
                container(
                    text(&task.filename)
                        .size(14)
                        .font(FONT_SEMIBOLD)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_primary),
                )
                .width(Length::Fill)
                .clip(true),
                resume_pill,
                threads_pill,
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            container(
                text(&task.resolved_url)
                    .size(11)
                    .font(FONT_MONO)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
            )
            .width(Length::Fill)
            .clip(true),
        ]
        .spacing(4)
        .width(Length::Fill),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let size_summary = match task.total_size {
        Some(total) => format!("{} / {}", format_bytes(task.downloaded_bytes), format_bytes(total)),
        None => format!("{} / 未知大小", format_bytes(task.downloaded_bytes)),
    };

    let bar_color = match task.status {
        TaskStatus::Error => palette.err,
        TaskStatus::Paused | TaskStatus::LinkExpired => palette.warn,
        _ => dl_green,
    };

    let overall_progress = column![
        row![
            text(size_summary)
                .size(12)
                .font(FONT_MONO)
                .color(palette.text_secondary),
            space::horizontal(),
            text(pct_str)
                .size(13)
                .font(FONT_MONO_BOLD)
                .color(bar_color),
        ]
        .align_y(Alignment::Center),
        progress_bar(0.0..=1.0, ratio)
            .girth(Length::Fixed(7.0))
            .style(progress_style(palette, bar_color)),
    ]
    .spacing(6);

    let detail_status_str = match task.status {
        TaskStatus::Downloading => "正在接收数据...",
        TaskStatus::Probing => "正在请求源站信息...",
        TaskStatus::Verifying => "正在校验哈希...",
        TaskStatus::Seeding => "已完成 · 正在做种",
        TaskStatus::Completed => "传输已完成",
        TaskStatus::Paused => "用户已暂停",
        TaskStatus::Queued => "队列等待中",
        TaskStatus::LinkExpired => "链接已失效",
        TaskStatus::Error => "连接中断",
    };

    let seeding_up_bps = task
        .bt_telemetry
        .as_ref()
        .map(|t| t.upload_speed_bps)
        .unwrap_or(0);

    let telemetry_grid = row![
        telemetry_cell("传输状态", detail_status_str.to_string(), palette.text_primary, palette),
        telemetry_cell(
            if task.status == TaskStatus::Seeding {
                "做种上传"
            } else {
                "实时速率"
            },
            if task.status == TaskStatus::Seeding {
                format!("↑ {}", format_speed(seeding_up_bps))
            } else if task.status.is_active() {
                format_speed(task.speed_bps)
            } else {
                "0 B/s".to_string()
            },
            if task.status == TaskStatus::Seeding {
                palette.ok
            } else if task.speed_bps > 0 {
                dl_green
            } else {
                palette.text_primary
            },
            palette,
        ),
        telemetry_cell(
            "剩余时间",
            if task.status.is_finished() {
                "00s".to_string()
            } else {
                format_eta(task.eta_secs)
            },
            palette.text_primary,
            palette,
        ),
    ]
    .spacing(8);

    let speed_chart = canvas(SpeedChart {
        samples: task.speed_history.clone(),
        palette: green_palette,
    })
    .width(Length::Fill)
    .height(Length::Fixed(76.0));

    let overview_card = container(
        column![
            file_header,
            overall_progress,
            telemetry_grid,
            speed_chart,
        ]
        .spacing(10),
    )
    .padding([12, 14])
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0));

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

    let segments_card = container(
        column![
            row![
                text("多线程分片进度")
                    .size(12)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                space::horizontal(),
                text(format!(
                    "{active_segs} 活跃 · {done_segs}/{} 分片",
                    task.segments.len()
                ))
                .size(11)
                .font(FONT_MONO)
                .color(if active_segs > 0 {
                    dl_green
                } else {
                    palette.text_muted
                }),
            ]
            .align_y(Alignment::Center),
            canvas(SegmentBar {
                segments: task.segments.clone(),
                total_size: task.total_size,
                palette: green_palette,
            })
            .width(Length::Fill)
            .height(Length::Fixed(16.0)),
        ]
        .spacing(8),
    )
    .padding([12, 14])
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0));

    let auto_close_chk = checkbox(popup.auto_close_on_complete)
        .label("下载完成后自动关闭此窗口")
        .on_toggle(move |v| Message::DownloadPopupToggleAutoClose(win_id, v))
        .size(15)
        .text_size(12)
        .font(FONT_REGULAR)
        .style(checkbox_style(palette, false));

    let action_buttons: Element<'a, Message> = if task.status.is_finished() {
        let left_btn = if task.status == TaskStatus::Seeding {
            button(
                container(
                    row![
                        icon(IconKind::Pause, 13.0, palette.warn),
                        text("停止做种")
                            .size(13)
                            .font(FONT_REGULAR)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(34.0))
            .padding([0, 14])
            .style(outline_btn_style(palette))
            .on_press(Message::PauseTask(task.id))
        } else {
            button(
                container(
                    text("关闭窗口")
                        .size(13)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(34.0))
            .padding([0, 14])
            .style(outline_btn_style(palette))
            .on_press(Message::DownloadPopupClose(win_id))
        };
        row![
            left_btn,
            button(
                container(
                    row![
                        icon(IconKind::Folder, 14.0, palette.text_secondary),
                        text("打开文件夹")
                            .size(13)
                            .font(FONT_REGULAR)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(34.0))
            .padding([0, 14])
            .style(outline_btn_style(palette))
            .on_press(Message::RevealTaskInExplorer(task.id)),
            button(
                container(
                    row![
                        icon(IconKind::Play, 13.0, palette.cta_text),
                        text("打开文件")
                            .size(13)
                            .font(FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(34.0))
            .padding([0, 16])
            .style(primary_btn_style(palette))
            .on_press(Message::DownloadPopupOpenFile(win_id, task.id)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    } else {
        let pause_or_resume = if task.status.is_active() || task.status == TaskStatus::Queued {
            button(
                container(
                    row![
                        icon(IconKind::Pause, 13.0, palette.text_primary),
                        text("暂停")
                            .size(13)
                            .font(FONT_REGULAR)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(34.0))
            .padding([0, 14])
            .style(outline_btn_style(palette))
            .on_press(Message::PauseTask(task.id))
        } else {
            button(
                container(
                    row![
                        icon(IconKind::Play, 13.0, palette.cta_text),
                        text("继续下载")
                            .size(13)
                            .font(FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(34.0))
            .padding([0, 15])
            .style(primary_btn_style(palette))
            .on_press(Message::ResumeTask(task.id))
        };

        row![
            button(
                container(
                    text("转入后台")
                        .size(13)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(34.0))
            .padding([0, 14])
            .style(outline_btn_style(palette))
            .on_press(Message::DownloadPopupClose(win_id)),
            pause_or_resume,
            button(
                container(
                    row![
                        icon(IconKind::Trash, 13.0, palette.err),
                        text("取消下载")
                            .size(13)
                            .font(FONT_REGULAR)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(Length::Fixed(34.0))
            .padding([0, 14])
            .style(danger_btn_style(palette))
            .on_press(Message::DownloadPopupCancelTask(win_id, task.id)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    };

    let footer = row![auto_close_chk, space::horizontal(), action_buttons]
        .spacing(8)
        .align_y(Alignment::Center);

    let body = column![overview_card, segments_card, space::vertical(), footer]
        .spacing(10)
        .padding([14, 18])
        .height(Length::Fill);

    let root = column![
        top_bar,
        rule::horizontal(1).style(hairline_rule_style(palette)),
        body,
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    container(root)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(panel_style(
            palette.bg_main,
            Some(palette.hairline_strong),
            0.0,
        ))
        .into()
}

fn telemetry_cell<'a>(
    label: &'static str,
    value: String,
    value_color: Color,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    container(
        column![
            text(label)
                .size(11)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None)
                .color(palette.text_muted),
            text(value)
                .size(13)
                .font(FONT_MONO_BOLD)
                .wrapping(text::Wrapping::None)
                .color(value_color),
        ]
        .spacing(2),
    )
    .width(Length::Fill)
    .padding([6, 10])
    .style(panel_style(palette.bg_input, None, 6.0))
    .into()
}

fn pill<'a>(
    label: String,
    fg: Color,
    bg: Color,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    container(
        text(label)
            .size(11)
            .font(FONT_MONO)
            .wrapping(text::Wrapping::None)
            .color(fg),
    )
    .padding([2, 8])
    .style(panel_style(bg, Some(palette.hairline), 5.0))
    .into()
}
