use crate::i18n::Strings;
use crate::icons::{IconKind, icon};
use crate::state::{Message, NavSection};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_MONO_BOLD, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette,
    clean_scrollable_style, danger_btn_style, outline_btn_style, panel_style, primary_btn_style,
    progress_style, subtle_btn_style, task_row_btn_style,
};
use iced::widget::{button, column, container, image, progress_bar, row, scrollable, space, text};
use iced::{Alignment, Color, Element, Length};
use siphon_core::{
    DownloadTask, ListDensity, ProtocolKind, TaskId, TaskStatus, format_bytes, format_eta,
    format_speed,
};
use siphon_platform_win::FileIconKey;
use std::collections::HashMap;

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    nav: NavSection,
    tasks: &[&'a DownloadTask],
    selected_id: Option<TaskId>,
    density: ListDensity,
    file_icons: &'a HashMap<FileIconKey, (u64, image::Handle)>,
    clipboard_url: Option<&'a str>,
    toast: Option<&'a str>,
    palette: SiphonPalette,
    strings: Strings,
) -> Element<'a, Message> {
    let title_str = match nav {
        NavSection::All => strings.all_tasks(),
        NavSection::Downloading => strings.active_tasks(),
        NavSection::Completed => strings.completed_tasks(),
        NavSection::Paused => strings.paused_tasks(),
        NavSection::Category(cat) => strings.category_label(cat),
        _ => strings.all_tasks(),
    };

    let count_caption = if strings.lang == siphon_core::Language::ZhCn {
        format!("{} 项", tasks.len())
    } else {
        format!("{} items", tasks.len())
    };

    // Segmented density switch inside a single quiet container
    let density_segment = container(
        row![
            button(
                text(strings.density_comfortable())
                    .size(12)
                    .wrapping(text::Wrapping::None)
                    .font(if density == ListDensity::Comfortable {
                        FONT_MEDIUM
                    } else {
                        FONT_REGULAR
                    })
            )
            .padding([5, 10])
            .style(subtle_btn_style(
                palette,
                density == ListDensity::Comfortable
            ))
            .on_press(Message::SetDensity(ListDensity::Comfortable)),
            button(
                text(strings.density_compact())
                    .size(12)
                    .wrapping(text::Wrapping::None)
                    .font(if density == ListDensity::Compact {
                        FONT_MEDIUM
                    } else {
                        FONT_REGULAR
                    })
            )
            .padding([5, 10])
            .style(subtle_btn_style(palette, density == ListDensity::Compact))
            .on_press(Message::SetDensity(ListDensity::Compact)),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    )
    .padding(2)
    .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0));

    let header = row![
        row![
            text(title_str)
                .size(22)
                .font(FONT_SEMIBOLD)
                .wrapping(text::Wrapping::None)
                .color(palette.text_primary),
            container(
                text(count_caption)
                    .size(13)
                    .font(FONT_MONO)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted)
            )
            .padding(iced::Padding {
                top: 3.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            }),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        space::horizontal(),
        row![
            density_segment,
            button(
                container(
                    row![
                        icon(IconKind::Pause, 12.0, palette.text_secondary),
                        text(strings.pause_all())
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
            .height(Length::Fixed(32.0))
            .padding([0, 12])
            .style(outline_btn_style(palette))
            .on_press(Message::PauseAll),
            button(
                container(
                    row![
                        icon(IconKind::Play, 12.0, palette.text_secondary),
                        text(strings.resume_all())
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
            .height(Length::Fixed(32.0))
            .padding([0, 12])
            .style(outline_btn_style(palette))
            .on_press(Message::ResumeAll),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .align_y(Alignment::Center)
    .padding(iced::Padding {
        top: 24.0,
        right: 28.0,
        bottom: 16.0,
        left: 28.0,
    });

    let mut content_col = column![header].spacing(0);

    // Non-intrusive clipboard link banner
    if let Some(url) = clipboard_url {
        let preview = if url.len() > 74 {
            format!("{}...", &url[..74])
        } else {
            url.to_string()
        };
        let banner = container(
            row![
                icon(IconKind::Link, 15.0, palette.accent),
                column![
                    text("检测到剪贴板直链")
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text(preview)
                        .size(12)
                        .font(FONT_MONO)
                        .color(palette.text_secondary),
                ]
                .spacing(2)
                .width(Length::Fill),
                button(text("立即下载").size(13).font(FONT_MEDIUM))
                    .padding([6, 12])
                    .style(primary_btn_style(palette))
                    .on_press(Message::ClipboardQuickDownload),
                button(text("配置").size(13).font(FONT_REGULAR))
                    .padding([6, 12])
                    .style(outline_btn_style(palette))
                    .on_press(Message::ClipboardOpenModal),
                button(icon(IconKind::Close, 13.0, palette.text_secondary))
                    .padding([6, 8])
                    .style(subtle_btn_style(palette, false))
                    .on_press(Message::DismissClipboardBanner),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([12, 16])
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline_strong), 10.0));
        content_col = content_col.push(container(banner).padding([0, 28]));
        content_col = content_col.push(space::vertical().height(12));
    }

    if let Some(msg) = toast {
        let toast_box = container(
            row![
                text(msg)
                    .size(13)
                    .font(FONT_REGULAR)
                    .color(palette.text_primary),
                space::horizontal(),
                button(icon(IconKind::Close, 12.0, palette.text_secondary))
                    .padding([4, 6])
                    .style(subtle_btn_style(palette, false))
                    .on_press(Message::ClearToast),
            ]
            .align_y(Alignment::Center),
        )
        .padding([10, 14])
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline_strong), 8.0));
        content_col = content_col.push(container(toast_box).padding([0, 28]));
        content_col = content_col.push(space::vertical().height(12));
    }

    if tasks.is_empty() {
        // Calm, architectural empty state — no duplicate bright CTA competing with the titlebar
        let empty = container(
            column![
                icon(IconKind::Brand, 36.0, palette.text_muted),
                space::vertical().height(6),
                text("还没有下载任务")
                    .size(17)
                    .font(FONT_MEDIUM)
                    .color(palette.text_primary),
                text("在顶部栏粘贴链接回车即可直接开下，或直接从浏览器拖入链接")
                    .size(14)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary),
                space::vertical().height(12),
                row![
                    shortcut_hint("Ctrl N", "配置新建", palette),
                    shortcut_hint("Ctrl K", "快下 / 搜索", palette),
                ]
                .spacing(20)
                .align_y(Alignment::Center),
                space::vertical().height(10),
                button(
                    container(
                        row![
                            icon(IconKind::Plus, 14.0, palette.text_primary),
                            text("从链接新建").size(14).font(FONT_MEDIUM),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .height(Length::Fill)
                    .align_y(Alignment::Center),
                )
                .height(Length::Fixed(36.0))
                .padding([0, 18])
                .style(outline_btn_style(palette))
                .on_press(Message::OpenNewTaskModal(None)),
            ]
            .spacing(6)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: 28.0,
            bottom: 48.0,
            left: 28.0,
        })
        .center_x(Length::Fill)
        .center_y(Length::Fill);

        content_col = content_col.push(empty);
    } else {
        let spacing = match density {
            ListDensity::Comfortable => 8,
            ListDensity::Compact => 4,
        };
        let mut list_col = column![]
            .spacing(spacing)
            .padding(iced::Padding {
                top: 4.0,
                right: 28.0,
                bottom: 24.0,
                left: 28.0,
            });
        for task in tasks {
            let is_selected = selected_id == Some(task.id);
            let icon_key = FileIconKey::for_task(
                &task.filename,
                &task.target_path(),
                task.status == TaskStatus::Completed,
            );
            let sys_icon = file_icons
                .get(&icon_key)
                .or_else(|| file_icons.get(&icon_key.fallback_extension()))
                .map(|(_, h)| h);
            list_col = list_col.push(task_row(
                task,
                is_selected,
                density,
                sys_icon,
                palette,
                strings,
            ));
        }

        content_col = content_col.push(
            scrollable(list_col)
                .height(Length::Fill)
                .style(clean_scrollable_style(palette)),
        );
    }

    container(content_col)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(panel_style(palette.bg_main, None, 0.0))
        .into()
}

fn shortcut_hint<'a>(
    keys: &'static str,
    desc: &'static str,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    row![
        container(
            text(keys)
                .size(12)
                .font(FONT_MONO)
                .color(palette.text_secondary)
        )
        .padding([3, 8])
        .style(panel_style(palette.bg_card, Some(palette.hairline), 6.0)),
        text(desc)
            .size(13)
            .font(FONT_REGULAR)
            .color(palette.text_muted),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn task_row<'a>(
    task: &'a DownloadTask,
    selected: bool,
    density: ListDensity,
    sys_icon: Option<&'a image::Handle>,
    palette: SiphonPalette,
    strings: Strings,
) -> Element<'a, Message> {
    let status_color = palette.status_color(task.status);
    let progress = task.progress_ratio();
    let pct_str = format!("{:>5.1}%", progress * 100.0);

    let size_summary = match task.total_size {
        Some(total) => format!(
            "{} / {}",
            format_bytes(task.downloaded_bytes),
            format_bytes(total)
        ),
        None => format_bytes(task.downloaded_bytes),
    };

    // Siphon signature 3px left accent indicator when selected or actively transferring
    let left_tick = container(space())
        .width(Length::Fixed(3.0))
        .height(Length::Fixed(if density == ListDensity::Compact {
            24.0
        } else {
            34.0
        }))
        .style(panel_style(
            if selected || task.status == TaskStatus::Downloading {
                palette.accent
            } else {
                Color::TRANSPARENT
            },
            None,
            2.0,
        ));

    let badge_dim = if density == ListDensity::Compact {
        30.0
    } else {
        36.0
    };
    let icon_inner: Element<'a, Message> = if let Some(handle) = sys_icon {
        let px = if density == ListDensity::Compact {
            20.0
        } else {
            24.0
        };
        image(handle.clone())
            .width(Length::Fixed(px))
            .height(Length::Fixed(px))
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
    let cat_badge = container(icon_inner)
        .center_x(Length::Fixed(badge_dim))
        .center_y(Length::Fixed(badge_dim))
        .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0));

    let status_dot = container(space())
        .width(Length::Fixed(6.0))
        .height(Length::Fixed(6.0))
        .style(panel_style(status_color, None, 3.0));

    let mut meta_row = row![status_dot]
        .spacing(6)
        .width(Length::Fill)
        .align_y(Alignment::Center);

    if matches!(
        task.protocol,
        ProtocolKind::Hls | ProtocolKind::BitTorrent | ProtocolKind::Ed2k
    ) {
        let (badge_txt, badge_fg, badge_bg) = match task.protocol {
            ProtocolKind::Hls => ("HLS", palette.ok, palette.ok_soft),
            ProtocolKind::BitTorrent => ("BT", palette.warn, palette.warn_soft),
            ProtocolKind::Ed2k => ("eD2k", palette.accent, palette.accent_soft),
            _ => unreachable!(),
        };
        meta_row = meta_row.push(
            container(
                text(badge_txt)
                    .size(10)
                    .font(FONT_MONO_BOLD)
                    .wrapping(text::Wrapping::None)
                    .color(badge_fg),
            )
            .padding([1, 5])
            .style(panel_style(badge_bg, None, 4.0)),
        );
    }

    meta_row = meta_row
        .push(
            text(strings.status_label(task.status))
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None)
                .color(palette.text_secondary),
        )
        .push(
            text("·")
                .size(12)
                .wrapping(text::Wrapping::None)
                .color(palette.text_muted),
        )
        .push(
            text(size_summary)
                .size(12)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::None)
                .color(palette.text_secondary),
        );

    if task.status == TaskStatus::Downloading {
        meta_row = meta_row
            .push(
                text("·")
                    .size(12)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
            )
            .push(
                text(format!("剩余 {}", format_eta(task.eta_secs)))
                    .size(12)
                    .font(FONT_MONO)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_secondary),
            );
    }

    let domain = task.domain();
    if !domain.is_empty() {
        meta_row = meta_row
            .push(
                text("·")
                    .size(12)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
            )
            .push(
                container(
                    text(domain)
                        .size(12)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_muted),
                )
                .width(Length::Fill)
                .clip(true),
            );
    }

    let metrics = if task.status == TaskStatus::Downloading {
        row![
            text(format_speed(task.speed_bps))
                .size(13)
                .font(FONT_MONO_BOLD)
                .wrapping(text::Wrapping::None)
                .color(palette.accent),
            text(pct_str)
                .size(12)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::None)
                .color(palette.text_secondary),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    } else {
        row![
            text(pct_str)
                .size(12)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::None)
                .color(palette.text_muted),
        ]
    };

    let action_btn = if task.status.is_active() || task.status == TaskStatus::Queued {
        button(icon(IconKind::Pause, 13.0, palette.text_secondary))
            .padding([6, 8])
            .style(subtle_btn_style(palette, false))
            .on_press(Message::PauseTask(task.id))
    } else if task.status == TaskStatus::Completed {
        button(icon(IconKind::Folder, 14.0, palette.text_secondary))
            .padding([6, 8])
            .style(subtle_btn_style(palette, false))
            .on_press(Message::RevealTaskInExplorer(task.id))
    } else {
        button(icon(IconKind::Play, 13.0, palette.text_primary))
            .padding([6, 8])
            .style(subtle_btn_style(palette, false))
            .on_press(Message::ResumeTask(task.id))
    };

    let del_btn = button(icon(IconKind::Trash, 13.0, palette.text_muted))
        .padding([6, 8])
        .style(danger_btn_style(palette))
        .on_press(Message::RequestDeleteTask(task.id));

    let bar = progress_bar(0.0..=1.0, progress)
        .girth(Length::Fixed(3.0))
        .style(progress_style(palette, status_color));

    let title_line = row![
        container(
            text(&task.filename)
                .size(if density == ListDensity::Compact {
                    14
                } else {
                    15
                })
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::None)
                .color(palette.text_primary),
        )
        .width(Length::Fill)
        .clip(true),
        metrics,
    ]
    .spacing(12)
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let center_col = if density == ListDensity::Compact {
        column![title_line, bar].spacing(6).width(Length::Fill)
    } else {
        column![title_line, meta_row, bar]
            .spacing(6)
            .width(Length::Fill)
    };

    let inner = row![
        left_tick,
        cat_badge,
        center_col,
        row![action_btn, del_btn]
            .spacing(4)
            .align_y(Alignment::Center),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let pad = if density == ListDensity::Compact {
        iced::Padding {
            top: 8.0,
            right: 12.0,
            bottom: 8.0,
            left: 6.0,
        }
    } else {
        iced::Padding {
            top: 12.0,
            right: 14.0,
            bottom: 12.0,
            left: 6.0,
        }
    };

    button(inner)
        .width(Length::Fill)
        .padding(pad)
        .style(task_row_btn_style(palette, selected))
        .on_press(Message::SelectTask(task.id))
        .into()
}
