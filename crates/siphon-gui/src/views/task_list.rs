use crate::i18n::Strings;
use crate::icons::{IconKind, icon};
use crate::state::{
    Message, NavSection, TaskListFilterState, TaskProtocolFilter, TaskQuickFilter, TaskSortBy,
    TorrentFilter, TorrentSummary,
};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_MONO_BOLD, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette,
    clean_scrollable_style, danger_btn_style, hairline_rule_style, outline_btn_style, panel_style,
    primary_btn_style, progress_style, subtle_btn_style, task_row_btn_style,
};
use iced::widget::{
    button, column, container, image, progress_bar, row, rule, scrollable, space, text, toggler,
};
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
    unfiltered_nav_count: usize,
    selected_id: Option<TaskId>,
    density: ListDensity,
    list_filter: TaskListFilterState,
    torrent_filter: TorrentFilter,
    torrent_summary: TorrentSummary,
    file_icons: &'a HashMap<FileIconKey, (u64, image::Handle)>,
    clipboard_url: Option<&'a str>,
    toast: Option<&'a str>,
    palette: SiphonPalette,
    strings: Strings,
) -> Element<'a, Message> {
    let is_torrents_view = nav == NavSection::Torrents;

    let title_str = match nav {
        NavSection::All => strings.all_tasks(),
        NavSection::Downloading => strings.active_tasks(),
        NavSection::Completed => strings.completed_tasks(),
        NavSection::Paused => strings.paused_tasks(),
        NavSection::Torrents => strings.torrents_title(),
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

    let header_actions: Element<'a, Message> = if is_torrents_view {
        let can_resume_dl = torrent_summary.bt_enabled && torrent_summary.paused_or_error > 0;
        let can_pause_dl = torrent_summary.active_or_queued > 0;
        row![
            density_segment,
            button(
                container(
                    row![
                        icon(
                            IconKind::Downloading,
                            12.0,
                            if can_resume_dl {
                                palette.accent
                            } else {
                                palette.text_muted
                            }
                        ),
                        text("全部下载")
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
            .on_press_maybe(can_resume_dl.then_some(Message::ResumeAllTorrents)),
            button(
                container(
                    row![
                        icon(
                            IconKind::Pause,
                            12.0,
                            if can_pause_dl {
                                palette.text_secondary
                            } else {
                                palette.text_muted
                            }
                        ),
                        text("全部暂停")
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
            .on_press_maybe(can_pause_dl.then_some(Message::PauseAllTorrents)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    } else {
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
        .align_y(Alignment::Center)
        .into()
    };

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
        header_actions,
    ]
    .align_y(Alignment::Center)
    .padding(iced::Padding {
        top: 24.0,
        right: 28.0,
        bottom: 16.0,
        left: 28.0,
    });

    let mut content_col = column![header].spacing(0);

    let sort_pill = |kind: TaskSortBy, label: &'static str| {
        let active = list_filter.sort_by == kind;
        button(
            text(label)
                .size(11)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                .wrapping(text::Wrapping::None),
        )
        .padding([4, 8])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::SetTaskSortBy(kind))
    };

    let pin_toggle_btn = button(
        row![
            container(space())
                .width(Length::Fixed(6.0))
                .height(Length::Fixed(6.0))
                .style(panel_style(
                    if list_filter.pin_downloading {
                        palette.accent
                    } else {
                        palette.text_muted
                    },
                    None,
                    3.0,
                )),
            text("下载置顶")
                .size(11)
                .font(if list_filter.pin_downloading {
                    FONT_MEDIUM
                } else {
                    FONT_REGULAR
                })
                .wrapping(text::Wrapping::None)
                .color(if list_filter.pin_downloading {
                    palette.accent
                } else {
                    palette.text_secondary
                }),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .style(subtle_btn_style(palette, list_filter.pin_downloading))
    .on_press(Message::TogglePinDownloading(!list_filter.pin_downloading));

    let dir_label = if list_filter.sort_by == TaskSortBy::Name {
        if list_filter.sort_desc { "A→Z" } else { "Z→A" }
    } else if list_filter.sort_desc {
        "↓ 降序"
    } else {
        "↑ 升序"
    };

    let sort_cluster = row![
        container(
            row![
                sort_pill(TaskSortBy::ProgressAndTime, "默认"),
                sort_pill(TaskSortBy::Progress, "进度"),
                sort_pill(TaskSortBy::CreatedAt, "时间"),
                sort_pill(TaskSortBy::Size, "大小"),
                sort_pill(TaskSortBy::Name, "名称"),
            ]
            .spacing(2)
            .align_y(Alignment::Center),
        )
        .padding(2)
        .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0)),
        button(
            text(dir_label)
                .size(11)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::None)
                .color(palette.text_secondary),
        )
        .padding([4, 7])
        .style(outline_btn_style(palette))
        .on_press(Message::ToggleTaskSortOrder),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // Dedicated Seeding Management Control Strip for the Torrents category
    if is_torrents_view {
        let filter_pill = |kind: TorrentFilter, label: String, dot_color: Option<Color>| {
            let active = torrent_filter == kind;
            let mut inner = row![].spacing(5).align_y(Alignment::Center);
            if let Some(c) = dot_color {
                inner = inner.push(
                    container(space())
                        .width(Length::Fixed(6.0))
                        .height(Length::Fixed(6.0))
                        .style(panel_style(c, None, 3.0)),
                );
            }
            inner = inner.push(
                text(label)
                    .size(12)
                    .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                    .wrapping(text::Wrapping::None),
            );
            button(inner)
                .padding([4, 10])
                .style(subtle_btn_style(palette, active))
                .on_press(Message::SetTorrentFilter(kind))
        };

        let filter_bar = container(
            row![
                filter_pill(
                    TorrentFilter::All,
                    format!("全部 ({})", torrent_summary.total),
                    None,
                ),
                filter_pill(
                    TorrentFilter::Seeding,
                    format!("做种中 ({})", torrent_summary.seeding),
                    (torrent_summary.seeding > 0).then_some(palette.ok),
                ),
                filter_pill(
                    TorrentFilter::Downloading,
                    format!("下载中 ({})", torrent_summary.downloading),
                    (torrent_summary.downloading > 0).then_some(palette.accent),
                ),
                filter_pill(
                    TorrentFilter::Stopped,
                    format!("已停种 ({})", torrent_summary.stopped),
                    None,
                ),
            ]
            .spacing(2)
            .align_y(Alignment::Center),
        )
        .padding(2)
        .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0));

        let can_start_all_seed = torrent_summary.bt_enabled && torrent_summary.completed_idle > 0;
        let can_stop_all_seed = torrent_summary.seeding > 0;

        let mut bottom_left = row![
            button(
                row![
                    icon(
                        IconKind::Play,
                        11.0,
                        if can_start_all_seed {
                            palette.ok
                        } else {
                            palette.text_muted
                        }
                    ),
                    text("全部开始做种")
                        .size(12)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([4, 10])
            .style(outline_btn_style(palette))
            .on_press_maybe(can_start_all_seed.then_some(Message::StartAllSeeding)),
            button(
                row![
                    icon(
                        IconKind::Pause,
                        11.0,
                        if can_stop_all_seed {
                            palette.warn
                        } else {
                            palette.text_muted
                        }
                    ),
                    text("全部停止做种")
                        .size(12)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([4, 10])
            .style(outline_btn_style(palette))
            .on_press_maybe(can_stop_all_seed.then_some(Message::StopAllSeeding)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        if !torrent_summary.bt_enabled {
            bottom_left = bottom_left.push(
                button(
                    text("开启 BT 引擎")
                        .size(11)
                        .font(FONT_MEDIUM)
                        .wrapping(text::Wrapping::None),
                )
                .padding([4, 9])
                .style(primary_btn_style(palette))
                .on_press(Message::BtToggleEnabled(true)),
            );
        }

        let seeding_bar = container(
            column![
                row![
                    filter_bar,
                    space::horizontal(),
                    row![
                        text(format!("↑ {}", format_speed(torrent_summary.total_upload_bps)))
                            .size(13)
                            .font(FONT_MONO_BOLD)
                            .wrapping(text::Wrapping::None)
                            .color(if torrent_summary.total_upload_bps > 0 || torrent_summary.seeding > 0 {
                                palette.ok
                            } else {
                                palette.text_secondary
                            }),
                        text("·")
                            .size(12)
                            .wrapping(text::Wrapping::None)
                            .color(palette.text_muted),
                        text(format!(
                            "累计上传 {}",
                            format_bytes(torrent_summary.total_uploaded_bytes)
                        ))
                        .size(12)
                        .font(FONT_MONO)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_secondary),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                rule::horizontal(1).style(hairline_rule_style(palette)),
                row![
                    bottom_left,
                    space::horizontal(),
                    row![
                        text("完成后自动做种")
                            .size(12)
                            .font(FONT_REGULAR)
                            .wrapping(text::Wrapping::None)
                            .color(palette.text_secondary),
                        toggler(torrent_summary.seed_after_complete)
                            .on_toggle(Message::BtToggleSeedAfterComplete)
                            .size(16),
                        button(
                            row![
                                icon(IconKind::Settings, 12.0, palette.text_secondary),
                                text("BT 设置")
                                    .size(11)
                                    .font(FONT_REGULAR)
                                    .wrapping(text::Wrapping::None),
                            ]
                            .spacing(4)
                            .align_y(Alignment::Center),
                        )
                        .padding([3, 8])
                        .style(outline_btn_style(palette))
                        .on_press(Message::SelectNav(NavSection::BitTorrent)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                rule::horizontal(1).style(hairline_rule_style(palette)),
                row![
                    pin_toggle_btn,
                    space::horizontal(),
                    sort_cluster,
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(10),
        )
        .padding([12, 14])
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0));

        content_col = content_col.push(container(seeding_bar).padding([0, 28]));
        content_col = content_col.push(space::vertical().height(12));
    } else {
        // Compact Filter & Sort Strip for All / Category / Status views
        let left_filter_rail: Element<'a, Message> = match nav {
            NavSection::All | NavSection::Category(_) => {
                let status_pill = |kind: TaskQuickFilter, label: &'static str| {
                    let active = list_filter.quick_filter == kind;
                    button(
                        text(label)
                            .size(11)
                            .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([4, 8])
                    .style(subtle_btn_style(palette, active))
                    .on_press(Message::SetTaskQuickFilter(kind))
                };
                container(
                    row![
                        status_pill(TaskQuickFilter::All, "全部"),
                        status_pill(TaskQuickFilter::Downloading, "下载"),
                        status_pill(TaskQuickFilter::Seeding, "做种"),
                        status_pill(TaskQuickFilter::Paused, "暂停"),
                        status_pill(TaskQuickFilter::Completed, "完成"),
                    ]
                    .spacing(2)
                    .align_y(Alignment::Center),
                )
                .padding(2)
                .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0))
                .into()
            }
            _ => {
                let proto_pill = |kind: TaskProtocolFilter, label: &'static str| {
                    let active = list_filter.protocol_filter == kind;
                    button(
                        text(label)
                            .size(11)
                            .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([4, 8])
                    .style(subtle_btn_style(palette, active))
                    .on_press(Message::SetTaskProtocolFilter(kind))
                };
                container(
                    row![
                        proto_pill(TaskProtocolFilter::All, "全部协议"),
                        proto_pill(TaskProtocolFilter::Http, "HTTP"),
                        proto_pill(TaskProtocolFilter::BitTorrent, "BT"),
                        proto_pill(TaskProtocolFilter::Hls, "HLS"),
                        proto_pill(TaskProtocolFilter::Ed2k, "eD2k"),
                    ]
                    .spacing(2)
                    .align_y(Alignment::Center),
                )
                .padding(2)
                .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0))
                .into()
            }
        };

        let mut right_sort_row = row![].spacing(6).align_y(Alignment::Center);
        if matches!(nav, NavSection::All | NavSection::Category(_)) {
            right_sort_row = right_sort_row.push(pin_toggle_btn);
        }
        right_sort_row = right_sort_row.push(sort_cluster);

        let filter_sort_bar = container(
            row![left_filter_rail, space::horizontal(), right_sort_row]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .padding([7, 10])
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 9.0));

        content_col = content_col.push(container(filter_sort_bar).padding([0, 28]));
        content_col = content_col.push(space::vertical().height(10));
    }

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
        let empty: Element<'a, Message> = if is_torrents_view && torrent_summary.total > 0 {
            container(
                column![
                    icon(IconKind::Magnet, 32.0, palette.text_muted),
                    space::vertical().height(6),
                    text("当前筛选下没有种子任务")
                        .size(16)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text("切换筛选标签即可查看其余做种、下载中或已停种任务")
                        .size(13)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                    space::vertical().height(10),
                    button(
                        text("查看全部种子")
                            .size(13)
                            .font(FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([8, 16])
                    .style(outline_btn_style(palette))
                    .on_press(Message::SetTorrentFilter(TorrentFilter::All)),
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
            .center_y(Length::Fill)
            .into()
        } else if is_torrents_view {
            container(
                column![
                    icon(IconKind::Magnet, 36.0, palette.text_muted),
                    space::vertical().height(6),
                    text("还没有种子 / 磁力任务")
                        .size(17)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text("在顶部栏粘贴 magnet:? 链接或直接拖入 .torrent 文件即可下载并在此管理做种")
                        .size(14)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                    space::vertical().height(12),
                    button(
                        container(
                            row![
                                icon(IconKind::Plus, 14.0, palette.text_primary),
                                text("新建种子 / 磁力下载")
                                    .size(14)
                                    .font(FONT_MEDIUM)
                                    .wrapping(text::Wrapping::None),
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
            .center_y(Length::Fill)
            .into()
        } else if !is_torrents_view
            && unfiltered_nav_count > 0
            && (list_filter.quick_filter != TaskQuickFilter::All
                || list_filter.protocol_filter != TaskProtocolFilter::All)
        {
            container(
                column![
                    icon(IconKind::Search, 32.0, palette.text_muted),
                    space::vertical().height(6),
                    text("当前筛选条件下没有匹配任务")
                        .size(16)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text("切换上方筛选条件或重置筛选即可显示全部任务")
                        .size(13)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                    space::vertical().height(10),
                    button(
                        text("重置筛选")
                            .size(13)
                            .font(FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([8, 16])
                    .style(outline_btn_style(palette))
                    .on_press(Message::ResetTaskFilters),
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
            .center_y(Length::Fill)
            .into()
        } else {
            // Calm, architectural empty state — no duplicate bright CTA competing with the titlebar
            container(
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
            .center_y(Length::Fill)
            .into()
        };

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
                task.status.is_finished(),
            );
            let sys_icon = file_icons
                .get(&icon_key)
                .or_else(|| file_icons.get(&icon_key.fallback_extension()))
                .map(|(_, h)| h);
            list_col = list_col.push(task_row(
                task,
                is_selected,
                is_torrents_view,
                torrent_summary.bt_enabled,
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

#[allow(clippy::too_many_arguments)]
fn task_row<'a>(
    task: &'a DownloadTask,
    selected: bool,
    torrents_mode: bool,
    bt_enabled: bool,
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
            } else if task.status == TaskStatus::Seeding {
                palette.ok
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
        let is_magnet = task
            .original_url
            .trim()
            .to_ascii_lowercase()
            .starts_with("magnet:?");
        let (badge_txt, badge_fg, badge_bg) = match task.protocol {
            ProtocolKind::Hls => ("HLS", palette.ok, palette.ok_soft),
            ProtocolKind::BitTorrent if is_magnet => ("MAGNET", palette.accent, palette.accent_soft),
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

    if task.protocol == ProtocolKind::BitTorrent
        && let Some(tel) = &task.bt_telemetry
        && task.status == TaskStatus::Seeding
    {
        let ratio = if task.downloaded_bytes > 0 {
            tel.uploaded_bytes as f64 / task.downloaded_bytes as f64
        } else {
            0.0
        };
        let seed_info = format!(
            "Peer {} · 已上传 {} ({:.2})",
            tel.live_peers,
            format_bytes(tel.uploaded_bytes),
            ratio
        );
        meta_row = meta_row
            .push(
                text("·")
                    .size(12)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
            )
            .push(
                container(
                    text(seed_info)
                        .size(12)
                        .font(FONT_MONO)
                        .wrapping(text::Wrapping::None)
                        .color(palette.ok),
                )
                .width(Length::Fill)
                .clip(true),
            );
    } else if task.protocol == ProtocolKind::BitTorrent
        && task.status == TaskStatus::Completed
        && torrents_mode
    {
        let uploaded = task
            .bt_telemetry
            .as_ref()
            .map(|t| t.uploaded_bytes)
            .unwrap_or(0);
        let ratio = if task.downloaded_bytes > 0 {
            uploaded as f64 / task.downloaded_bytes as f64
        } else {
            0.0
        };
        let done_seed_note = if uploaded > 0 {
            format!("已上传 {} ({:.2}) · 已停种", format_bytes(uploaded), ratio)
        } else {
            "已下载完成 · 待做种".to_string()
        };
        meta_row = meta_row
            .push(
                text("·")
                    .size(12)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
            )
            .push(
                container(
                    text(done_seed_note)
                        .size(12)
                        .font(FONT_MONO)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_muted),
                )
                .width(Length::Fill)
                .clip(true),
            );
    } else if task.protocol == ProtocolKind::BitTorrent
        && let Some(tel) = &task.bt_telemetry
        && task.status.is_active()
    {
        let peer_info = if tel.live_ipv6 > 0 {
            format!("Peer {}/{} (v6:{})", tel.live_peers, tel.seen_peers, tel.live_ipv6)
        } else {
            format!("Peer {}/{}", tel.live_peers, tel.seen_peers)
        };
        meta_row = meta_row
            .push(
                text("·")
                    .size(12)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
            )
            .push(
                container(
                    text(peer_info)
                        .size(12)
                        .font(FONT_MONO)
                        .wrapping(text::Wrapping::None)
                        .color(if tel.live_peers > 0 {
                            palette.accent
                        } else {
                            palette.warn
                        }),
                )
                .width(Length::Fill)
                .clip(true),
            );
    } else {
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
    } else if task.status == TaskStatus::Seeding {
        let up_bps = task
            .bt_telemetry
            .as_ref()
            .map(|t| t.upload_speed_bps)
            .unwrap_or(0);
        row![
            text(format!("↑ {}", format_speed(up_bps)))
                .size(13)
                .font(FONT_MONO_BOLD)
                .wrapping(text::Wrapping::None)
                .color(palette.ok),
            text("做种中")
                .size(12)
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::None)
                .color(palette.ok),
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

    let del_btn = button(icon(IconKind::Trash, 13.0, palette.text_muted))
        .padding([6, 8])
        .style(danger_btn_style(palette))
        .on_press(Message::RequestDeleteTask(task.id));

    let row_actions: Element<'a, Message> = if torrents_mode && task.status == TaskStatus::Seeding {
        row![
            button(
                row![
                    icon(IconKind::Pause, 11.0, palette.warn),
                    text("停止做种")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([4, 9])
            .style(outline_btn_style(palette))
            .on_press(Message::PauseTask(task.id)),
            button(icon(IconKind::Folder, 14.0, palette.text_secondary))
                .padding([5, 7])
                .style(subtle_btn_style(palette, false))
                .on_press(Message::RevealTaskInExplorer(task.id)),
            del_btn,
        ]
        .width(Length::Shrink)
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    } else if torrents_mode
        && task.status == TaskStatus::Completed
        && task.protocol == ProtocolKind::BitTorrent
    {
        row![
            button(
                row![
                    icon(
                        IconKind::Play,
                        11.0,
                        if bt_enabled {
                            palette.ok
                        } else {
                            palette.text_muted
                        }
                    ),
                    text("开始做种")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([4, 9])
            .style(outline_btn_style(palette))
            .on_press_maybe(bt_enabled.then_some(Message::ResumeTask(task.id))),
            button(icon(IconKind::Folder, 14.0, palette.text_secondary))
                .padding([5, 7])
                .style(subtle_btn_style(palette, false))
                .on_press(Message::RevealTaskInExplorer(task.id)),
            del_btn,
        ]
        .width(Length::Shrink)
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    } else {
        let action_btn = if task.status.is_active()
            || task.status == TaskStatus::Queued
            || task.status == TaskStatus::Seeding
        {
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
        row![action_btn, del_btn]
            .width(Length::Shrink)
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
    };

    let bar = progress_bar(0.0..=1.0, progress)
        .length(Length::Fill)
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

    let inner: Element<'a, Message> = if torrents_mode {
        // Re-arranged card layout for Torrents view:
        // Actions sit inline beside metadata/title, and the progress bar spans cleanly underneath
        // at full card width so wide seeding action buttons never touch or overlap the progress bar.
        let top_section = if density == ListDensity::Compact {
            row![
                left_tick,
                cat_badge,
                container(title_line).width(Length::Fill).clip(true),
                row_actions,
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(Alignment::Center)
        } else {
            let meta_and_actions = row![
                container(meta_row).width(Length::Fill).clip(true),
                row_actions,
            ]
            .spacing(10)
            .width(Length::Fill)
            .align_y(Alignment::Center);

            row![
                left_tick,
                cat_badge,
                column![title_line, meta_and_actions]
                    .spacing(6)
                    .width(Length::Fill),
            ]
            .spacing(12)
            .width(Length::Fill)
            .align_y(Alignment::Center)
        };

        column![top_section, bar]
            .spacing(if density == ListDensity::Compact { 6 } else { 8 })
            .width(Length::Fill)
            .into()
    } else {
        let center_col = if density == ListDensity::Compact {
            column![title_line, bar].spacing(6).width(Length::Fill)
        } else {
            column![title_line, meta_row, bar]
                .spacing(6)
                .width(Length::Fill)
        };

        row![
            left_tick,
            cat_badge,
            container(center_col).width(Length::Fill).clip(true),
            row_actions,
        ]
        .spacing(12)
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .into()
    };

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
