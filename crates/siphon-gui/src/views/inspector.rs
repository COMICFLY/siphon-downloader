use crate::i18n::Strings;
use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message, NavSection, P2pSelectTab};
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
use iced::{Alignment, Color, Element, Length};
use siphon_core::{
    BtEncryptionMode, BtPeerBrief, DownloadTask, ProtocolKind, SegmentInfo, SegmentState,
    TaskStatus, format_bytes, format_eta, format_speed,
};
use siphon_engine_bt::{BtPortCheckReport, BtPortLight, BtSessionStatusSnapshot};

pub fn replace_url_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-replace-url")
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    task: &'a DownloadTask,
    file_icon: Option<&'a image::Handle>,
    show_replace_url: bool,
    replace_url_input: &'a str,
    bt_status: &'a BtSessionStatusSnapshot,
    bt_port_report: Option<&'a BtPortCheckReport>,
    bt_checking_port: bool,
    palette: SiphonPalette,
    strings: Strings,
) -> Element<'a, Message> {
    if task.protocol == ProtocolKind::BitTorrent {
        return view_bt_inspector(
            task,
            bt_status,
            bt_port_report,
            bt_checking_port,
            palette,
            strings,
        );
    }
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

/// Extract info-hash from live BT telemetry or fallback to parsing `xt=urn:btih:...` from magnet URI.
fn extract_bt_info_hash(task: &DownloadTask) -> Option<String> {
    if let Some(tel) = &task.bt_telemetry
        && !tel.info_hash.trim().is_empty()
    {
        return Some(tel.info_hash.trim().to_ascii_lowercase());
    }
    let lower = task.original_url.to_ascii_lowercase();
    if let Some(pos) = lower.find("urn:btih:") {
        let rest = &task.original_url[pos + 9..];
        let hash: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if hash.len() >= 16 {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

fn short_hash(hash: &str) -> String {
    if hash.len() <= 16 {
        hash.to_string()
    } else {
        format!("{}…{}", &hash[..8], &hash[hash.len() - 6..])
    }
}

/// Specialized right-hand inspector card exclusively for Magnet & BitTorrent tasks,
/// visually distinct from ordinary HTTP/HLS downloads with live Swarm connection metrics,
/// port reachability diagnostics, dual-direction speed, share ratio, sub-file selection,
/// and live connected Peer list.
fn view_bt_inspector<'a>(
    task: &'a DownloadTask,
    bt_status: &'a BtSessionStatusSnapshot,
    bt_port_report: Option<&'a BtPortCheckReport>,
    bt_checking_port: bool,
    palette: SiphonPalette,
    strings: Strings,
) -> Element<'a, Message> {
    let is_magnet = task
        .original_url
        .trim()
        .to_ascii_lowercase()
        .starts_with("magnet:?");
    let tel = task.bt_telemetry.as_ref();

    // Header: visually marked as Magnet / BT Swarm Inspector
    let header = row![
        row![
            icon(IconKind::Magnet, 14.0, palette.accent),
            text(if is_magnet {
                "磁力 Swarm 详情"
            } else {
                "种子 Swarm 详情"
            })
            .size(13)
            .font(FONT_SEMIBOLD)
            .color(palette.accent),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
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

    // Distinct Protocol Identity Card (accent-tinted badge + InfoHash copy bar)
    let status_color = palette.status_color(task.status);
    let phase_label = if task.status.is_active() {
        tel.map(|t| t.phase.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| strings.status_label(task.status))
    } else {
        strings.status_label(task.status)
    };

    let info_hash = extract_bt_info_hash(task);
    let mut hash_row = row![
        text(match &info_hash {
            Some(h) => format!("BTIH {}", short_hash(h)),
            None => {
                if is_magnet {
                    "BTIH 解析中".to_string()
                } else {
                    "本地 .torrent 种子".to_string()
                }
            }
        })
        .size(11)
        .font(FONT_MONO)
        .color(palette.text_secondary),
        space::horizontal(),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if let Some(h) = &info_hash {
        hash_row = hash_row.push(
            button(text("复制哈希").size(11).font(FONT_REGULAR))
                .padding([2, 7])
                .style(outline_btn_style(palette))
                .on_press(Message::CopyTextToClipboard(h.clone())),
        );
    }
    hash_row = hash_row.push(
        button(text("复制源链接").size(11).font(FONT_REGULAR))
            .padding([2, 7])
            .style(outline_btn_style(palette))
            .on_press(Message::CopyTextToClipboard(task.original_url.clone())),
    );

    let title_card = container(
        column![
            row![
                container(icon(IconKind::Magnet, 19.0, palette.accent))
                    .center_x(Length::Fixed(38.0))
                    .center_y(Length::Fixed(38.0))
                    .style(panel_style(palette.accent_soft, Some(palette.accent), 9.0)),
                column![
                    text(&task.filename)
                        .size(15)
                        .font(FONT_SEMIBOLD)
                        .wrapping(text::Wrapping::WordOrGlyph)
                        .color(palette.text_primary),
                    row![
                        container(
                            text(if is_magnet { "MAGNET 磁力" } else { "TORRENT 种子" })
                                .size(10)
                                .font(FONT_MONO_BOLD)
                                .color(palette.accent),
                        )
                        .padding([2, 6])
                        .style(panel_style(palette.accent_soft, None, 4.0)),
                        container(space())
                            .width(Length::Fixed(6.0))
                            .height(Length::Fixed(6.0))
                            .style(panel_style(status_color, None, 3.0)),
                        text(phase_label)
                            .size(12)
                            .font(FONT_MEDIUM)
                            .wrapping(text::Wrapping::None)
                            .color(palette.text_secondary),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                ]
                .spacing(5)
                .width(Length::Fill),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            hash_row,
        ]
        .spacing(10),
    )
    .padding(14)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.accent), 12.0));

    // Card 1: Real-Time Swarm Connection & Port Reachability Card
    let incoming_any = bt_status.incoming_observed
        || tel.is_some_and(|t| t.incoming_peers > 0);
    let effective_light = match bt_port_report.map(|r| r.light) {
        Some(BtPortLight::Red) => BtPortLight::Red,
        Some(BtPortLight::Green) => BtPortLight::Green,
        Some(BtPortLight::Yellow) if incoming_any => BtPortLight::Green,
        Some(BtPortLight::Yellow) => BtPortLight::Yellow,
        _ if incoming_any => BtPortLight::Green,
        _ => BtPortLight::Unknown,
    };

    let (light_color, light_bg, light_label) = if bt_checking_port {
        (palette.accent, palette.accent_soft, "端口探测中...")
    } else {
        match effective_light {
            BtPortLight::Green => (palette.ok, palette.ok_soft, "绿灯 · 入站畅通"),
            BtPortLight::Yellow => (palette.warn, palette.warn_soft, "黄灯 · 仅出站受限"),
            BtPortLight::Red => (palette.err, palette.err_soft, "红灯 · 监听异常"),
            BtPortLight::Unknown => (palette.text_secondary, palette.bg_input, "端口未检测"),
        }
    };

    let dual_stack = tel.map(|t| t.dual_stack).unwrap_or(bt_status.dual_stack);
    let listen_port = tel
        .map(|t| t.listen_port)
        .filter(|&p| p > 0)
        .unwrap_or(bt_status.listen_port);
    let listen_addr = tel
        .map(|t| t.listen_addr.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| bt_status.listen_addr.clone())
        .or_else(|| bt_port_report.map(|r| r.listen_addr.clone()))
        .unwrap_or_else(|| {
            if listen_port > 0 {
                format!(":{listen_port}")
            } else {
                "预热中".to_string()
            }
        });

    let live_peers = tel.map(|t| t.live_peers).unwrap_or(0);
    let live_seeders = tel.map(|t| t.live_seeders).unwrap_or(0);
    let incoming_peers = tel.map(|t| t.incoming_peers).unwrap_or(0);
    let live_tcp = tel.map(|t| t.live_tcp).unwrap_or(0);
    let live_utp = tel.map(|t| t.live_utp).unwrap_or(0);
    let live_ipv6 = tel.map(|t| t.live_ipv6).unwrap_or(0);
    let seen_peers = tel.map(|t| t.seen_peers).unwrap_or(0);
    let connecting_peers = tel.map(|t| t.connecting_peers).unwrap_or(0);
    let dead_peers = tel.map(|t| t.dead_peers).unwrap_or(0);
    let dht_v4 = tel
        .map(|t| t.dht_nodes_v4)
        .filter(|&n| n > 0)
        .unwrap_or(bt_status.dht_nodes_v4);
    let dht_v6 = tel
        .map(|t| t.dht_nodes_v6)
        .filter(|&n| n > 0)
        .unwrap_or(bt_status.dht_nodes_v6);
    let tracker_count = tel.map(|t| t.tracker_count).unwrap_or(0);

    let mut check_port_btn = button(
        row![
            icon(IconKind::Refresh, 11.0, palette.text_primary),
            text(if bt_checking_port { "检测中" } else { "测端口" })
                .size(11)
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding([4, 9])
    .style(outline_btn_style(palette));
    if !bt_checking_port {
        check_port_btn = check_port_btn.on_press(Message::BtCheckPort);
    }

    let bt_metric = |title: &'static str, val: String, val_color: Color, sub: String| {
        container(
            column![
                text(title)
                    .size(11)
                    .font(FONT_REGULAR)
                    .color(palette.text_muted),
                text(val).size(13).font(FONT_MONO_BOLD).color(val_color),
                text(sub)
                    .size(10)
                    .font(FONT_MONO)
                    .color(palette.text_secondary),
            ]
            .spacing(2),
        )
        .width(Length::Fill)
        .padding([7, 9])
        .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0))
    };

    let enc_mode = bt_port_report
        .map(|r| r.encryption_mode)
        .unwrap_or(bt_status.encryption_mode);
    let (enc_tag, enc_color) = match enc_mode {
        BtEncryptionMode::PreferEncrypted => ("MSE 优先加密", palette.ok),
        BtEncryptionMode::ForceEncrypted => ("MSE 强制加密", palette.ok),
        BtEncryptionMode::Disabled => ("明文模式", palette.warn),
    };
    let detected_tun = bt_port_report
        .and_then(|r| r.detected_tun_adapter.as_deref())
        .or(bt_status.detected_tun_adapter.as_deref());

    let mut tech_badges = row![
        container(
            text(format!(
                "{}{}",
                if dual_stack { "双栈" } else { "仅v4" },
                if listen_port > 0 {
                    format!(" :{listen_port}")
                } else {
                    String::new()
                }
            ))
            .size(11)
            .font(FONT_MONO)
            .wrapping(text::Wrapping::None)
            .color(if dual_stack {
                palette.accent
            } else {
                palette.text_secondary
            }),
        )
        .padding([3, 7])
        .style(panel_style(palette.bg_input, Some(palette.hairline), 6.0)),
        container(
            text(enc_tag)
                .size(11)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::None)
                .color(enc_color),
        )
        .padding([3, 7])
        .style(panel_style(palette.bg_input, Some(palette.hairline), 6.0)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if let Some(tun) = detected_tun {
        tech_badges = tech_badges.push(
            container(
                text(format!("TUN {tun}"))
                    .size(10)
                    .font(FONT_MONO)
                    .wrapping(text::Wrapping::None)
                    .color(palette.warn),
            )
            .padding([3, 7])
            .style(panel_style(palette.bg_input, Some(palette.hairline), 6.0)),
        );
    }

    let mut swarm_col = column![
        row![
            container(
                row![
                    container(space())
                        .width(Length::Fixed(7.0))
                        .height(Length::Fixed(7.0))
                        .style(panel_style(light_color, None, 4.0)),
                    text(light_label)
                        .size(12)
                        .font(FONT_MEDIUM)
                        .wrapping(text::Wrapping::None)
                        .color(light_color),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([4, 9])
            .style(panel_style(light_bg, None, 6.0)),
            space::horizontal(),
            check_port_btn,
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        tech_badges,
        row![
            bt_metric(
                "活跃连接",
                format!("{live_peers} Peer"),
                if live_peers > 0 {
                    palette.accent
                } else {
                    palette.text_primary
                },
                format!("做种 {live_seeders} · 入站 {incoming_peers}"),
            ),
            bt_metric(
                "传输协议",
                format!("TCP {live_tcp} / uTP {live_utp}"),
                if live_ipv6 > 0 {
                    palette.ok
                } else {
                    palette.text_primary
                },
                format!("IPv6 活跃 {live_ipv6} 个"),
            ),
        ]
        .spacing(8),
        row![
            bt_metric(
                "节点发现",
                format!("已见 {seen_peers}"),
                if seen_peers > 0 && live_peers == 0 && task.status.is_active() {
                    palette.warn
                } else {
                    palette.text_primary
                },
                format!("握手 {connecting_peers} · 失效 {dead_peers}"),
            ),
            bt_metric(
                "DHT / Tracker",
                format!("DHT {}", dht_v4 + dht_v6),
                if dht_v4 + dht_v6 > 0 {
                    palette.ok
                } else {
                    palette.text_primary
                },
                format!("v4:{dht_v4} v6:{dht_v6} · Trk {tracker_count}"),
            ),
        ]
        .spacing(8),
    ]
    .spacing(9);

    // Smart live diagnostic banner tailored to current Swarm + port status
    let diag_banner: Option<(Color, Color, String, String)> = if effective_light == BtPortLight::Red {
        if let Some(rep) = bt_port_report {
            Some((
                palette.err,
                palette.err_soft,
                rep.summary.clone(),
                rep.advice.clone(),
            ))
        } else {
            Some((
                palette.err,
                palette.err_soft,
                "本地 BT 监听端口异常".to_string(),
                "请检查端口是否被占用，或在「BT 与 Tracker」页更换固定端口后重试。".to_string(),
            ))
        }
    } else if task.status.is_active() && seen_peers > 0 && live_peers == 0 {
        let advice = if let Some(tun) = detected_tun {
            if enc_mode == BtEncryptionMode::Disabled {
                format!(
                    "检测到本机虚拟网卡（{tun}）且当前关闭了协议加密，明文 BT 握手易被直接掐断；请在下方「BT 端口与 Tracker 设置」开启 MSE/PE 加密或将 siphon.exe 加入 TUN 绕过规则。"
                )
            } else {
                format!(
                    "检测到本机虚拟网卡（{tun}），正在使用 MSE/PE 加密握手尝试连接；如需更高连上率，建议将 siphon.exe 加入 TUN 直连白名单。"
                )
            }
        } else {
            match effective_light {
                BtPortLight::Yellow => "当前为黄灯（仅出站）：已启用 MSE/PE 加密握手；若连上率仍低，建议开启路由器 UPnP / 端口转发并确认公网 IPv6 可用。".to_string(),
                BtPortLight::Green => "入站端口已畅通，正在与对端进行加密握手（冷门种子可能需要多等待片刻）。".to_string(),
                _ => "建议点击上方「测端口」检测本机监听、IPv6 与外网端口可达性，排查是否受 NAT 或防火墙阻塞。".to_string(),
            }
        };
        Some((
            palette.warn,
            palette.warn_soft,
            format!("已扫到 {seen_peers} 个在线节点，但活跃连接为 0"),
            advice,
        ))
    } else if effective_light == BtPortLight::Yellow {
        if let Some(rep) = bt_port_report {
            Some((
                palette.warn,
                palette.warn_soft,
                rep.summary.clone(),
                rep.advice.clone(),
            ))
        } else {
            None
        }
    } else if effective_light == BtPortLight::Green {
        Some((
            palette.ok,
            palette.ok_soft,
            format!("入站端口畅通（{listen_addr}）"),
            "可双向主动接受外网 Peer 连入，利于提升连上率与 Swarm 速率。".to_string(),
        ))
    } else {
        Some((
            palette.text_secondary,
            palette.bg_input,
            format!("当前监听：{listen_addr}"),
            "点击右上角「测端口」实时检测本机监听、公网 IPv6 与外网端口可达性。".to_string(),
        ))
    };

    if let Some((fg, bg, headline, detail)) = diag_banner {
        swarm_col = swarm_col.push(
            container(
                column![
                    text(headline).size(12).font(FONT_MEDIUM).color(fg),
                    text(detail)
                        .size(11)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                ]
                .spacing(3),
            )
            .padding([8, 10])
            .width(Length::Fill)
            .style(panel_style(bg, Some(palette.hairline), 7.0)),
        );
    }

    let swarm_card = container(swarm_col)
        .padding(14)
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 2: Dual-Direction Speed Chart (Download + Upload + Share Ratio)
    let upload_bps = tel.map(|t| t.upload_speed_bps).unwrap_or(0);
    let uploaded_bytes = tel.map(|t| t.uploaded_bytes).unwrap_or(0);
    let share_ratio = if task.downloaded_bytes > 0 {
        uploaded_bytes as f64 / task.downloaded_bytes as f64
    } else {
        0.0
    };

    let speed_card = container(
        column![
            row![
                text("双向吞吐（60秒）")
                    .size(12)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                space::horizontal(),
                text(format!("↓ {}", format_speed(task.speed_bps)))
                    .size(13)
                    .font(FONT_MONO_BOLD)
                    .color(if task.speed_bps > 0 {
                        palette.accent
                    } else {
                        palette.text_primary
                    }),
                text(format!("↑ {}", format_speed(upload_bps)))
                    .size(12)
                    .font(FONT_MONO)
                    .color(if upload_bps > 0 {
                        palette.ok
                    } else {
                        palette.text_muted
                    }),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            canvas(SpeedChart {
                samples: task.speed_history.clone(),
                palette,
            })
            .width(Length::Fill)
            .height(Length::Fixed(96.0)),
            row![
                stat_cell("已下载", format_bytes(task.downloaded_bytes), palette),
                stat_cell(
                    "总大小",
                    task.total_size
                        .map(format_bytes)
                        .unwrap_or_else(|| "解析中".into()),
                    palette
                ),
            ]
            .spacing(8),
            row![
                stat_cell(
                    "已上传 / 分享率",
                    format!("{} ({:.2})", format_bytes(uploaded_bytes), share_ratio),
                    palette
                ),
                stat_cell("预计剩余", format_eta(task.eta_secs), palette),
            ]
            .spacing(8),
        ]
        .spacing(10),
    )
    .padding(14)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 3: Swarm Piece Map & Selected Sub-Files
    let piece_label = match tel {
        Some(t) if t.total_pieces > 0 => {
            format!("已校验 {} / {} 片", t.checked_pieces, t.total_pieces)
        }
        _ => format!("{:.1}%", task.progress_ratio() * 100.0),
    };
    let subfile_summary = match &task.selected_files {
        Some(idxs) if !idxs.is_empty() => format!("仅下载勾选的 {} 个子文件", idxs.len()),
        _ => "下载种子全部文件".to_string(),
    };

    let pieces_card = container(
        column![
            row![
                text("Swarm 分片与子文件")
                    .size(12)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                space::horizontal(),
                text(piece_label)
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
            row![
                text(subfile_summary)
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary)
                    .width(Length::Fill),
                button(
                    row![
                        icon(IconKind::Layers, 12.0, palette.accent),
                        text("查看/重选文件").size(11).font(FONT_MEDIUM),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(outline_btn_style(palette))
                .on_press(Message::OpenP2pSelectPopup(
                    P2pSelectTab::Torrent,
                    Some(task.original_url.clone()),
                )),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(10),
    )
    .padding(14)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 4: Live Connected Peers Mini-List
    let top_peers: &[BtPeerBrief] = tel.map(|t| t.top_peers.as_slice()).unwrap_or(&[]);
    let mut peers_col = column![
        row![
            text(format!("已连入节点（{}）", top_peers.len()))
                .size(12)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
            space::horizontal(),
            text("栈 · 协议 · 方向 · 流量")
                .size(10)
                .font(FONT_MONO)
                .color(palette.text_muted),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(6);

    if top_peers.is_empty() {
        peers_col = peers_col.push(
            text(if task.status.is_active() {
                "正在与 Peer 建立连接并拉取数据，连上后将在此实时列出活跃节点..."
            } else {
                "任务当前未传输，恢复下载后可实时查看已连入节点列表"
            })
            .size(11)
            .font(FONT_REGULAR)
            .color(palette.text_muted),
        );
    } else {
        for peer in top_peers.iter().take(6) {
            peers_col = peers_col.push(peer_mini_row(peer, palette));
        }
    }

    let peers_card = container(peers_col)
        .padding(14)
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 5: BT-Tailored Speed Limit & Save Dir (NO HTTP concurrent-threads slider!)
    let limit_btn = |label: &'static str, bps: u64| {
        let active = task.speed_limit_bps == bps;
        button(text(label).size(12).font(FONT_MONO))
            .padding([4, 9])
            .style(subtle_btn_style(palette, active))
            .on_press(Message::SetTaskSpeedLimit(task.id, bps))
    };

    let controls_card = container(
        column![
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

    let mut error_section = column![].spacing(8);
    if let Some(err) = &task.error_message {
        error_section = error_section.push(
            container(
                text(err)
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.err),
            )
            .padding(12)
            .width(Length::Fill)
            .style(panel_style(palette.status_soft_bg(task.status), None, 8.0)),
        );
    }

    // BT Actions (NO HTTP "replace direct link" — replaced with direct jump to BT Port & Tracker Settings)
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
                        icon(IconKind::Settings, 13.0, palette.accent),
                        text("BT 端口与 Tracker 设置")
                            .size(12)
                            .font(FONT_REGULAR),
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
            .on_press(Message::SelectNav(NavSection::BitTorrent)),
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
        text("BT Swarm 实时日志")
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
        title_card,
        swarm_card,
        speed_card,
        pieces_card,
        peers_card,
        error_section,
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

fn peer_mini_row<'a>(peer: &'a BtPeerBrief, palette: SiphonPalette) -> Element<'a, Message> {
    let stack_label = if peer.is_ipv6 { "v6" } else { "v4" };
    let stack_color = if peer.is_ipv6 {
        palette.accent
    } else {
        palette.text_muted
    };
    let dir_label = if peer.incoming { "↙入" } else { "↗出" };
    let dir_color = if peer.incoming {
        palette.ok
    } else {
        palette.text_muted
    };
    let display_name = if !peer.client.trim().is_empty() {
        peer.client.trim().to_string()
    } else {
        peer.addr.clone()
    };
    let traffic = if peer.uploaded_bytes > 0 {
        format!(
            "↓{} ↑{}",
            format_bytes(peer.downloaded_bytes),
            format_bytes(peer.uploaded_bytes)
        )
    } else {
        format!("↓{}", format_bytes(peer.downloaded_bytes))
    };

    container(
        row![
            container(text(stack_label).size(10).font(FONT_MONO).color(stack_color))
                .padding([1, 4])
                .style(panel_style(palette.bg_card, Some(palette.hairline), 4.0)),
            container(
                text(peer.transport.as_str())
                    .size(10)
                    .font(FONT_MONO)
                    .color(palette.text_secondary),
            )
            .padding([1, 4])
            .style(panel_style(palette.bg_card, Some(palette.hairline), 4.0)),
            text(dir_label).size(10).font(FONT_MONO).color(dir_color),
            container(
                text(display_name)
                    .size(11)
                    .font(FONT_MONO)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_primary),
            )
            .width(Length::Fill)
            .clip(true),
            text(traffic)
                .size(10)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::None)
                .color(palette.text_secondary),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .padding([5, 7])
    .width(Length::Fill)
    .style(panel_style(palette.bg_input, Some(palette.hairline), 6.0))
    .into()
}
