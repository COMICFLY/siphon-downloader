use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message, SniffEntry, SniffFilterKind, SniffPopupState};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_MONO_BOLD, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette,
    checkbox_style, clean_scrollable_style, danger_btn_style, hairline_rule_style, input_style,
    outline_btn_style, panel_style, primary_btn_style, subtle_btn_style,
};
use iced::widget::{
    button, checkbox, column, container, mouse_area, row, rule, scrollable, slider, space, text,
    text_input,
};
use iced::{Alignment, Color, Element, Length};
use siphon_core::format_bytes;

pub fn sniff_page_url_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-sniff-page-url")
}

fn wrap_input<'a>(
    input: impl Into<Element<'a, Message>>,
    target: InputTarget,
) -> Element<'a, Message> {
    mouse_area(input)
        .on_release(Message::FocusInputTarget(target))
        .on_right_press(Message::OpenInputContextMenu(target))
        .into()
}

pub fn visible_entries(sniff: &SniffPopupState) -> Vec<&SniffEntry> {
    let q = sniff.search.trim().to_ascii_lowercase();
    sniff
        .items
        .iter()
        .filter(|item| sniff.filter.matches(&item.kind))
        .filter(|item| {
            if q.is_empty() {
                true
            } else {
                item.filename.to_ascii_lowercase().contains(&q)
                    || item.label.to_ascii_lowercase().contains(&q)
                    || item.url.to_ascii_lowercase().contains(&q)
            }
        })
        .collect()
}

pub fn view<'a>(sniff: &'a SniffPopupState, palette: SiphonPalette) -> Element<'a, Message> {
    let visible = visible_entries(sniff);
    let selected_count = sniff.items.iter().filter(|i| i.selected).count();
    let known_selected_bytes: u64 = sniff
        .items
        .iter()
        .filter(|i| i.selected)
        .filter_map(|i| i.file_size)
        .sum();
    let unknown_visible_count = visible
        .iter()
        .filter(|i| i.file_size.is_none() && (i.url.starts_with("http://") || i.url.starts_with("https://")))
        .count();
    let visible_all_selected = !visible.is_empty() && visible.iter().all(|i| i.selected);

    let page_caption = if !sniff.page_title.trim().is_empty() {
        sniff.page_title.as_str()
    } else if !sniff.page_url.trim().is_empty() {
        sniff.page_url.as_str()
    } else {
        "支持浏览器实时嗅探或粘贴任意网页链接提取资源"
    };

    // Custom frameless draggable titlebar
    let mut title_left = row![
        icon(IconKind::Brand, 17.0, palette.accent),
        text("Siphon · 嗅探资源选择性下载")
            .size(14)
            .font(FONT_SEMIBOLD)
            .color(palette.text_primary)
            .wrapping(text::Wrapping::None),
        container(
            text(format!("已选 {selected_count} / {} 项", sniff.items.len()))
                .size(11)
                .font(FONT_MONO_BOLD)
                .color(palette.accent)
                .wrapping(text::Wrapping::None),
        )
        .padding([2, 8])
        .style(panel_style(palette.accent_soft, None, 5.0)),
    ]
    .spacing(9)
    .align_y(Alignment::Center);

    if known_selected_bytes > 0 {
        title_left = title_left.push(
            container(
                text(format!("预计 {}", format_bytes(known_selected_bytes)))
                    .size(11)
                    .font(FONT_MONO)
                    .color(palette.ok)
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 8])
            .style(panel_style(palette.ok_soft, None, 5.0)),
        );
    }

    title_left = title_left.push(
        container(
            text(page_caption)
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted)
                .wrapping(text::Wrapping::None),
        )
        .width(Length::Fill)
        .clip(true),
    );

    let titlebar = container(
        row![
            mouse_area(title_left.width(Length::Fill)).on_press(Message::SniffDragWindow),
            button(icon(IconKind::Close, 13.0, palette.text_secondary))
                .padding([6, 8])
                .style(subtle_btn_style(palette, false))
                .on_press(Message::SniffClose),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([10, 14])
    .width(Length::Fill)
    .style(panel_style(palette.bg_chrome, None, 0.0));

    // Direct webpage URL sniffer input bar
    let can_fetch = !sniff.sniffing_page && !sniff.page_url_input.trim().is_empty();
    let url_sniff_bar = row![
        icon(IconKind::Link, 14.0, palette.accent),
        wrap_input(
            text_input(
                "粘贴任意网页链接 (https://...) 直接嗅探页面音视频、流媒体、文件、磁力与 eD2k...",
                &sniff.page_url_input
            )
            .id(sniff_page_url_id())
            .on_input(Message::SniffPageUrlChanged)
            .on_submit(Message::SniffFetchPageUrl)
            .font(FONT_REGULAR)
            .size(12)
            .padding([7, 10])
            .width(Length::Fill)
            .style(input_style(palette)),
            InputTarget::SniffPageUrl,
        ),
        button(
            row![
                icon(IconKind::Search, 12.0, palette.cta_text),
                text(if sniff.sniffing_page {
                    "嗅探中..."
                } else {
                    "嗅探网页"
                })
                .size(12)
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::None),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([7, 12])
        .style(primary_btn_style(palette))
        .on_press_maybe(can_fetch.then_some(Message::SniffFetchPageUrl)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Category filter chips with live counts
    let filter_btn = |kind: SniffFilterKind, title: &'static str| {
        let count = sniff.items.iter().filter(|i| kind.matches(&i.kind)).count();
        let active = sniff.filter == kind;
        let label = if kind == SniffFilterKind::All || count > 0 {
            format!("{title} ({count})")
        } else {
            title.to_string()
        };
        button(
            text(label)
                .size(12)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                .wrapping(text::Wrapping::None),
        )
        .padding([5, 10])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::SniffSetFilter(kind))
    };

    let filter_bar = row![
        filter_btn(SniffFilterKind::All, "全部"),
        filter_btn(SniffFilterKind::Hls, "HLS / MPD"),
        filter_btn(SniffFilterKind::Video, "视频"),
        filter_btn(SniffFilterKind::Audio, "音频"),
        filter_btn(SniffFilterKind::File, "文件"),
        filter_btn(SniffFilterKind::P2p, "磁力/eD2k"),
        filter_btn(SniffFilterKind::Image, "图片"),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let toolbar = row![
        wrap_input(
            text_input("按文件名、类型或链接关键词过滤...", &sniff.search)
                .on_input(Message::SniffSearchChanged)
                .font(FONT_REGULAR)
                .size(12)
                .padding([6, 10])
                .width(Length::Fill)
                .style(input_style(palette)),
            InputTarget::SniffSearch,
        ),
        button(
            text(if visible_all_selected {
                "取消全选"
            } else {
                "全选当前"
            })
            .size(12)
            .font(FONT_REGULAR)
            .wrapping(text::Wrapping::None),
        )
        .padding([6, 10])
        .style(outline_btn_style(palette))
        .on_press_maybe(
            (!visible.is_empty()).then_some(Message::SniffSelectAllVisible(!visible_all_selected))
        ),
        button(
            text("反选")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([6, 10])
        .style(outline_btn_style(palette))
        .on_press_maybe((!visible.is_empty()).then_some(Message::SniffInvertSelection)),
        button(
            text(if sniff.probing_sizes {
                "探测中..."
            } else {
                "探测大小"
            })
            .size(12)
            .font(FONT_REGULAR)
            .wrapping(text::Wrapping::None),
        )
        .padding([6, 10])
        .style(outline_btn_style(palette))
        .on_press_maybe(
            (!sniff.probing_sizes && unknown_visible_count > 0).then_some(Message::SniffProbeSizes)
        ),
        button(
            text("清空")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([6, 10])
        .style(danger_btn_style(palette))
        .on_press_maybe((!sniff.items.is_empty()).then_some(Message::SniffClearAll)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // Scrollable selectable checklist of sniffed items
    let list_body: Element<'a, Message> = if visible.is_empty() {
        container(
            column![
                icon(IconKind::Search, 28.0, palette.text_muted),
                text(if sniff.items.is_empty() {
                    "暂无待选择资源"
                } else {
                    "当前筛选条件下没有匹配的资源"
                })
                .size(14)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
                text("可在上方粘贴网页链接直接嗅探，或在浏览器扩展中点击「扫描当前页」自动同步流媒体与直链")
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_muted),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    } else {
        let mut rows_col = column![].spacing(6).padding([2, 4]);
        for entry in visible {
            let (tag_txt, tag_fg, tag_bg) = kind_badge_style(&entry.kind, palette);
            let title_str = if !entry.label.is_empty() && entry.label != entry.kind {
                format!("{}  ·  {}", entry.filename, entry.label)
            } else {
                entry.filename.clone()
            };
            let url_preview = truncate_chars(&entry.url, 86);

            let size_label: Element<'a, Message> = if let Some(sz) = entry.file_size {
                text(format_bytes(sz))
                    .size(11)
                    .font(FONT_MONO_BOLD)
                    .color(palette.text_secondary)
                    .wrapping(text::Wrapping::None)
                    .into()
            } else {
                text("未知大小")
                    .size(11)
                    .font(FONT_MONO)
                    .color(palette.text_muted)
                    .wrapping(text::Wrapping::None)
                    .into()
            };

            let meta_line = row![
                container(
                    text(tag_txt)
                        .size(10)
                        .font(FONT_MONO_BOLD)
                        .color(tag_fg)
                        .wrapping(text::Wrapping::None),
                )
                .padding([2, 6])
                .style(panel_style(tag_bg, None, 4.0)),
                container(
                    text(title_str)
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary)
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::Fill)
                .clip(true),
                size_label,
            ]
            .spacing(7)
            .align_y(Alignment::Center);

            let entry_id = entry.id;
            let row_card = container(
                row![
                    checkbox(entry.selected)
                        .on_toggle(move |_| Message::SniffToggleItem(entry_id))
                        .size(16)
                        .style(checkbox_style(palette, false)),
                    column![
                        meta_line,
                        text(url_preview)
                            .size(11)
                            .font(FONT_MONO)
                            .color(palette.text_muted)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(3)
                    .width(Length::Fill),
                    button(
                        row![
                            icon(IconKind::Downloading, 12.0, palette.text_primary),
                            text("下载")
                                .size(12)
                                .font(FONT_MEDIUM)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([5, 10])
                    .style(outline_btn_style(palette))
                    .on_press(Message::SniffDownloadSingle(entry_id)),
                    button(icon(IconKind::Close, 12.0, palette.text_muted))
                        .padding([5, 7])
                        .style(subtle_btn_style(palette, false))
                        .on_press(Message::SniffRemoveItem(entry_id)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([8, 11])
            .width(Length::Fill)
            .style(panel_style(
                if entry.selected {
                    palette.bg_card
                } else {
                    palette.bg_input
                },
                Some(if entry.selected {
                    palette.accent
                } else {
                    palette.hairline
                }),
                8.0,
            ));

            rows_col = rows_col.push(row_card);
        }

        scrollable(rows_col)
            .height(Length::Fill)
            .style(clean_scrollable_style(palette))
            .into()
    };

    // Footer: save directory + thread concurrency selector + batch selective actions
    let dir_row = row![
        text("保存到")
            .size(12)
            .font(FONT_MEDIUM)
            .color(palette.text_secondary)
            .wrapping(text::Wrapping::None),
        wrap_input(
            text_input("默认按规则目录保存", &sniff.save_dir)
                .on_input(Message::SniffSaveDirChanged)
                .font(FONT_MONO)
                .size(12)
                .padding([6, 10])
                .width(Length::Fill)
                .style(input_style(palette)),
            InputTarget::SniffSaveDir,
        ),
        button(
            row![
                icon(IconKind::Folder, 13.0, palette.text_secondary),
                text("浏览")
                    .size(12)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([6, 11])
        .style(outline_btn_style(palette))
        .on_press(Message::SniffBrowseDir),
        container(rule::vertical(1).style(hairline_rule_style(palette)))
            .width(Length::Fixed(6.0))
            .height(Length::Fixed(18.0))
            .center_x(Length::Fixed(6.0)),
        text(format!("{} 线程", sniff.connections))
            .size(12)
            .font(FONT_MONO)
            .color(palette.text_secondary)
            .wrapping(text::Wrapping::None),
        slider(
            1.0_f32..=32.0_f32,
            sniff.connections as f32,
            Message::SniffConnectionsChanged
        )
        .step(1.0_f32)
        .width(Length::Fixed(86.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let session_hint = if sniff.headers.contains_key("Cookie") {
        "已携带浏览器 Cookie 与 Referer"
    } else if sniff.headers.contains_key("Referer") {
        "已同步页面 Referer 与 User-Agent"
    } else {
        "支持勾选任意资源后批量下载或加入队列"
    };

    let action_row = row![
        text(session_hint)
            .size(11)
            .font(FONT_REGULAR)
            .color(palette.text_muted),
        space::horizontal(),
        button(text("关闭").size(13).font(FONT_REGULAR))
            .padding([7, 14])
            .style(outline_btn_style(palette))
            .on_press(Message::SniffClose),
        button(
            text(format!("加入暂停队列 ({selected_count})"))
                .size(13)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([7, 14])
        .style(outline_btn_style(palette))
        .on_press_maybe((selected_count > 0).then_some(Message::SniffDownloadSelected(false))),
        button(
            row![
                icon(IconKind::Downloading, 14.0, palette.cta_text),
                text(format!("下载选中资源 ({selected_count})"))
                    .size(13)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([7, 16])
        .style(primary_btn_style(palette))
        .on_press_maybe((selected_count > 0).then_some(Message::SniffDownloadSelected(true))),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut body_col = column![url_sniff_bar].spacing(9).height(Length::Fill);

    if let Some(err) = &sniff.sniff_error {
        body_col = body_col.push(
            container(
                text(err)
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.err),
            )
            .padding([6, 10])
            .width(Length::Fill)
            .style(panel_style(palette.err_soft, Some(palette.err), 6.0)),
        );
    }

    body_col = body_col
        .push(filter_bar)
        .push(toolbar)
        .push(
            container(list_body)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding([2, 0]),
        )
        .push(rule::horizontal(1).style(hairline_rule_style(palette)))
        .push(dir_row)
        .push(action_row);

    let content = column![
        titlebar,
        rule::horizontal(1).style(hairline_rule_style(palette)),
        container(body_col)
            .padding([12, 16])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(panel_style(palette.bg_main, None, 0.0)),
    ]
    .height(Length::Fill);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(panel_style(
            palette.bg_window,
            Some(palette.hairline_strong),
            0.0,
        ))
        .into()
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let head: String = chars.by_ref().take(max_chars.saturating_sub(3)).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        s.to_string()
    }
}

fn kind_badge_style(kind: &str, palette: SiphonPalette) -> (&'static str, Color, Color) {
    match kind.to_ascii_lowercase().as_str() {
        "hls" => ("HLS", palette.ok, palette.ok_soft),
        "video" => ("VIDEO", palette.accent, palette.accent_soft),
        "audio" => ("AUDIO", palette.accent, palette.accent_soft),
        "magnet" => ("MAGNET", palette.warn, palette.warn_soft),
        "torrent" => ("BT", palette.warn, palette.warn_soft),
        "ed2k" => ("eD2k", palette.accent, palette.accent_soft),
        "image" => ("IMG", palette.text_secondary, palette.bg_card),
        _ => ("FILE", palette.text_secondary, palette.bg_card),
    }
}
