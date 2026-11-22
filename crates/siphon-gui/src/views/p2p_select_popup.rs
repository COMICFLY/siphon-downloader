use crate::icons::{IconKind, icon};
use crate::state::{
    Ed2kSelectItem, InputTarget, Message, P2pFileFilter, P2pSelectPopupState, P2pSelectTab,
    TorrentSelectItem,
};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_MONO_BOLD, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette,
    checkbox_style, clean_scrollable_style, danger_btn_style, hairline_rule_style, input_style,
    outline_btn_style, panel_style, primary_btn_style, subtle_btn_style,
};
use iced::widget::{
    button, checkbox, column, container, mouse_area, row, rule, scrollable, space, text, text_input,
};
use iced::{Alignment, Color, Element, Length};
use siphon_core::{FileCategory, format_bytes};

const ED2K_CHUNK_SIZE: u64 = 9_728_000;

pub fn p2p_source_input_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-p2p-source-input")
}

pub fn p2p_ed2k_input_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-p2p-ed2k-input")
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

pub fn visible_bt_files(p2p: &P2pSelectPopupState) -> Vec<&TorrentSelectItem> {
    let q = p2p.search.trim().to_ascii_lowercase();
    p2p.bt_files
        .iter()
        .filter(|f| p2p.filter.matches_filename(&f.path))
        .filter(|f| q.is_empty() || f.path.to_ascii_lowercase().contains(&q))
        .collect()
}

pub fn visible_ed2k_items(p2p: &P2pSelectPopupState) -> Vec<&Ed2kSelectItem> {
    let q = p2p.search.trim().to_ascii_lowercase();
    p2p.ed2k_items
        .iter()
        .filter(|it| p2p.filter.matches_filename(&it.filename))
        .filter(|it| {
            q.is_empty()
                || it.filename.to_ascii_lowercase().contains(&q)
                || it.md4_hash.to_ascii_lowercase().contains(&q)
        })
        .collect()
}

pub fn view<'a>(
    p2p: &'a P2pSelectPopupState,
    ed2k_verify_md4: bool,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let (sel_count, total_count, sel_bytes) = match p2p.tab {
        P2pSelectTab::Torrent => {
            let sc = p2p.bt_files.iter().filter(|f| f.selected).count();
            let sb: u64 = p2p
                .bt_files
                .iter()
                .filter(|f| f.selected)
                .map(|f| f.size)
                .sum();
            (sc, p2p.bt_files.len(), sb)
        }
        P2pSelectTab::Ed2k => {
            let sc = p2p.ed2k_items.iter().filter(|i| i.selected).count();
            let sb: u64 = p2p
                .ed2k_items
                .iter()
                .filter(|i| i.selected)
                .map(|i| i.file_size)
                .sum();
            (sc, p2p.ed2k_items.len(), sb)
        }
    };

    // Segmented mode switch in titlebar
    let tab_btn = |tab: P2pSelectTab, label: &'static str, icon_kind: IconKind| {
        let active = p2p.tab == tab;
        button(
            row![
                icon(
                    icon_kind,
                    12.0,
                    if active {
                        palette.accent
                    } else {
                        palette.text_muted
                    }
                ),
                text(label)
                    .size(12)
                    .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([4, 10])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::P2pSetTab(tab))
    };

    let tab_switch = container(
        row![
            tab_btn(P2pSelectTab::Torrent, "BT 种子 / 磁力", IconKind::Magnet),
            tab_btn(P2pSelectTab::Ed2k, "eD2k 电驴", IconKind::Server),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    )
    .padding(2)
    .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0));

    let mut summary_pills = row![
        container(
            text(format!("已选 {sel_count} / {total_count}"))
                .size(11)
                .font(FONT_MONO_BOLD)
                .color(palette.accent)
                .wrapping(text::Wrapping::None),
        )
        .padding([2, 8])
        .style(panel_style(palette.accent_soft, None, 5.0)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if sel_bytes > 0 {
        summary_pills = summary_pills.push(
            container(
                text(format!("预计 {}", format_bytes(sel_bytes)))
                    .size(11)
                    .font(FONT_MONO)
                    .color(palette.ok)
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 8])
            .style(panel_style(palette.ok_soft, None, 5.0)),
        );
    }

    let titlebar = container(
        row![
            mouse_area(
                row![
                    icon(IconKind::Brand, 17.0, palette.accent),
                    text("Siphon · 种子 / eD2k 选择下载")
                        .size(14)
                        .font(FONT_SEMIBOLD)
                        .color(palette.text_primary)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(9)
                .align_y(Alignment::Center),
            )
            .on_press(Message::P2pDragWindow),
            tab_switch,
            mouse_area(container(space()).width(Length::Fill).height(Length::Fixed(28.0)))
                .on_press(Message::P2pDragWindow),
            summary_pills,
            button(icon(IconKind::Close, 13.0, palette.text_secondary))
                .padding([6, 8])
                .style(subtle_btn_style(palette, false))
                .on_press(Message::P2pClose),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([9, 14])
    .width(Length::Fill)
    .style(panel_style(palette.bg_chrome, None, 0.0));

    let body: Element<'a, Message> = match p2p.tab {
        P2pSelectTab::Torrent => view_torrent_tab(p2p, palette),
        P2pSelectTab::Ed2k => view_ed2k_tab(p2p, ed2k_verify_md4, palette),
    };

    let content = column![
        titlebar,
        rule::horizontal(1).style(hairline_rule_style(palette)),
        container(body)
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

fn view_torrent_tab<'a>(
    p2p: &'a P2pSelectPopupState,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let visible = visible_bt_files(p2p);
    let selected_count = p2p.bt_files.iter().filter(|f| f.selected).count();
    let selected_bytes: u64 = p2p
        .bt_files
        .iter()
        .filter(|f| f.selected)
        .map(|f| f.size)
        .sum();
    let total_bytes: u64 = p2p.bt_files.iter().map(|f| f.size).sum();
    let visible_all_selected = !visible.is_empty() && visible.iter().all(|f| f.selected);
    let has_source = !p2p.source_input.trim().is_empty();

    // Source input + pick local .torrent + inspect
    let source_bar = row![
        icon(IconKind::Magnet, 14.0, palette.accent),
        wrap_input(
            text_input(
                "粘贴 magnet:? 磁力链接、.torrent 直链，或点击右侧选择本地 .torrent 种子文件...",
                &p2p.source_input
            )
            .id(p2p_source_input_id())
            .on_input(Message::P2pSourceInputChanged)
            .on_submit(Message::P2pInspectTorrent)
            .font(FONT_REGULAR)
            .size(12)
            .padding([7, 10])
            .width(Length::Fill)
            .style(input_style(palette)),
            InputTarget::P2pSourceInput,
        ),
        button(
            row![
                icon(IconKind::Folder, 12.0, palette.text_secondary),
                text("选择 .torrent")
                    .size(12)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([7, 11])
        .style(outline_btn_style(palette))
        .on_press(Message::P2pPickTorrentFile),
        button(
            text(if p2p.inspecting_bt {
                "解析中..."
            } else {
                "解析文件列表"
            })
            .size(12)
            .font(FONT_MEDIUM)
            .wrapping(text::Wrapping::None),
        )
        .padding([7, 12])
        .style(primary_btn_style(palette))
        .on_press_maybe((!p2p.inspecting_bt && has_source).then_some(Message::P2pInspectTorrent)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut col = column![source_bar].spacing(9).height(Length::Fill);

    if let Some(err) = &p2p.bt_error {
        let err_row = row![
            text(err)
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.err)
                .width(Length::Fill),
            button(
                text("不等待列表，直接全部下载")
                    .size(12)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
            )
            .padding([5, 10])
            .style(outline_btn_style(palette))
            .on_press_maybe(has_source.then_some(Message::P2pSubmitBtDirect(true))),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        col = col.push(
            container(err_row)
                .padding([7, 10])
                .width(Length::Fill)
                .style(panel_style(palette.err_soft, Some(palette.err), 6.0)),
        );
    }

    // Metadata banner when inspected
    if !p2p.bt_files.is_empty() {
        let short_hash = if p2p.info_hash.len() > 16 {
            format!("{}…", &p2p.info_hash[..16])
        } else {
            p2p.info_hash.clone()
        };
        let meta_card = container(
            row![
                icon(IconKind::Archive, 15.0, palette.accent),
                column![
                    text(&p2p.torrent_name)
                        .size(13)
                        .font(FONT_SEMIBOLD)
                        .color(palette.text_primary)
                        .wrapping(text::Wrapping::None),
                    text(format!(
                        "InfoHash: {short_hash}  ·  共 {} 个文件（总大小 {}）  ·  当前勾选 {} 个（{}）",
                        p2p.bt_files.len(),
                        format_bytes(total_bytes),
                        selected_count,
                        format_bytes(selected_bytes),
                    ))
                    .size(11)
                    .font(FONT_MONO)
                    .color(palette.text_muted),
                ]
                .spacing(2)
                .width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 8.0));

        col = col.push(meta_card);
    }

    // Filter chips
    let filter_btn = |kind: P2pFileFilter, label: &'static str| {
        let count = p2p
            .bt_files
            .iter()
            .filter(|f| kind.matches_filename(&f.path))
            .count();
        let active = p2p.filter == kind;
        let caption = if kind == P2pFileFilter::All || count > 0 {
            format!("{label} ({count})")
        } else {
            label.to_string()
        };
        button(
            text(caption)
                .size(12)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                .wrapping(text::Wrapping::None),
        )
        .padding([5, 10])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::P2pSetFilter(kind))
    };

    let filter_bar = row![
        filter_btn(P2pFileFilter::All, "全部"),
        filter_btn(P2pFileFilter::Video, "视频"),
        filter_btn(P2pFileFilter::Audio, "音频"),
        filter_btn(P2pFileFilter::ArchiveIso, "压缩包/镜像"),
        filter_btn(P2pFileFilter::DocImage, "文档/图片"),
        filter_btn(P2pFileFilter::Other, "其他"),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let toolbar = row![
        wrap_input(
            text_input("按子文件名或目录关键词筛选...", &p2p.search)
                .on_input(Message::P2pSearchChanged)
                .font(FONT_REGULAR)
                .size(12)
                .padding([6, 10])
                .width(Length::Fill)
                .style(input_style(palette)),
            InputTarget::P2pSearch,
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
            (!visible.is_empty()).then_some(Message::P2pSelectAllVisible(!visible_all_selected))
        ),
        button(
            text("反选")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([6, 10])
        .style(outline_btn_style(palette))
        .on_press_maybe((!visible.is_empty()).then_some(Message::P2pInvertSelection)),
        button(
            text("仅选大文件 (>10MB)")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([6, 10])
        .style(outline_btn_style(palette))
        .on_press_maybe((!p2p.bt_files.is_empty()).then_some(Message::P2pBtSelectLargeOnly)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // File list
    let list_body: Element<'a, Message> = if p2p.inspecting_bt {
        container(
            column![
                icon(IconKind::Magnet, 28.0, palette.accent),
                text("正在通过预热共享会话获取种子文件列表...")
                    .size(14)
                    .font(FONT_MEDIUM)
                    .color(palette.text_primary),
                text("本地 .torrent 秒级解析；磁力链接会并发通过持久化 DHT 与 16 组 Tracker 拉取元数据")
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_muted),
                button(
                    text("不等待文件列表，直接全部开始下载")
                        .size(12)
                        .font(FONT_MEDIUM),
                )
                .padding([7, 14])
                .style(outline_btn_style(palette))
                .on_press_maybe(has_source.then_some(Message::P2pSubmitBtDirect(true))),
            ]
            .spacing(10)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    } else if visible.is_empty() {
        container(
            column![
                icon(IconKind::Magnet, 28.0, palette.text_muted),
                text(if p2p.bt_files.is_empty() {
                    "选择本地 .torrent 文件或粘贴 magnet:? 磁力链接开始挑选子文件"
                } else {
                    "当前筛选条件下没有匹配的子文件"
                })
                .size(14)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
                text("支持勾选任意子文件按需下载，未勾选的文件不会占用磁盘空间")
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
        let mut rows_col = column![].spacing(5).padding([2, 4]);
        for file in visible {
            let (tag_txt, tag_fg, tag_bg) = category_badge_for_path(&file.path, palette);
            let (fname, folder_hint) = split_display_path(&file.path);
            let file_idx = file.index;

            let mut name_col = column![
                row![
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
                        text(fname)
                            .size(13)
                            .font(FONT_MEDIUM)
                            .color(palette.text_primary)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::Fill)
                    .clip(true),
                    text(format!("#{file_idx}"))
                        .size(10)
                        .font(FONT_MONO)
                        .color(palette.text_muted)
                        .wrapping(text::Wrapping::None),
                    text(format_bytes(file.size))
                        .size(12)
                        .font(FONT_MONO_BOLD)
                        .color(palette.text_secondary)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
            ]
            .spacing(2)
            .width(Length::Fill);

            if !folder_hint.is_empty() {
                name_col = name_col.push(
                    text(folder_hint)
                        .size(11)
                        .font(FONT_MONO)
                        .color(palette.text_muted)
                        .wrapping(text::Wrapping::None),
                );
            }

            let card = container(
                row![
                    checkbox(file.selected)
                        .on_toggle(move |_| Message::P2pToggleBtFile(file_idx))
                        .size(16)
                        .style(checkbox_style(palette, false)),
                    name_col,
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([7, 11])
            .width(Length::Fill)
            .style(panel_style(
                if file.selected {
                    palette.bg_card
                } else {
                    palette.bg_input
                },
                Some(if file.selected {
                    palette.accent
                } else {
                    palette.hairline
                }),
                7.0,
            ));

            rows_col = rows_col.push(card);
        }

        scrollable(rows_col)
            .height(Length::Fill)
            .style(clean_scrollable_style(palette))
            .into()
    };

    let dir_row = build_save_dir_row(p2p, palette);

    let action_row = if !p2p.bt_files.is_empty() {
        row![
            text(format!(
                "仅下载勾选的 {selected_count} / {} 个子文件（{}）",
                p2p.bt_files.len(),
                format_bytes(selected_bytes)
            ))
            .size(11)
            .font(FONT_REGULAR)
            .color(palette.text_muted),
            space::horizontal(),
            button(text("关闭").size(13).font(FONT_REGULAR))
                .padding([7, 14])
                .style(outline_btn_style(palette))
                .on_press(Message::P2pClose),
            button(
                text(format!("加入暂停队列 ({selected_count})"))
                    .size(13)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None),
            )
            .padding([7, 14])
            .style(outline_btn_style(palette))
            .on_press_maybe((selected_count > 0).then_some(Message::P2pSubmitBtSelected(false))),
            button(
                row![
                    icon(IconKind::Downloading, 14.0, palette.cta_text),
                    text(format!("下载选中文件 ({selected_count})"))
                        .size(13)
                        .font(FONT_MEDIUM)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([7, 16])
            .style(primary_btn_style(palette))
            .on_press_maybe((selected_count > 0).then_some(Message::P2pSubmitBtSelected(true))),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    } else {
        row![
            text("支持 .torrent 种子文件秒级展开，或对磁力链接直接开始下载")
                .size(11)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
            space::horizontal(),
            button(text("关闭").size(13).font(FONT_REGULAR))
                .padding([7, 14])
                .style(outline_btn_style(palette))
                .on_press(Message::P2pClose),
            button(
                text("不解析直接加入队列")
                    .size(13)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None),
            )
            .padding([7, 14])
            .style(outline_btn_style(palette))
            .on_press_maybe(has_source.then_some(Message::P2pSubmitBtDirect(false))),
            button(
                row![
                    icon(IconKind::Downloading, 14.0, palette.cta_text),
                    text("不解析直接开始下载")
                        .size(13)
                        .font(FONT_MEDIUM)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([7, 16])
            .style(primary_btn_style(palette))
            .on_press_maybe(has_source.then_some(Message::P2pSubmitBtDirect(true))),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    };

    col.push(filter_bar)
        .push(toolbar)
        .push(
            container(list_body)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding([2, 0]),
        )
        .push(rule::horizontal(1).style(hairline_rule_style(palette)))
        .push(dir_row)
        .push(action_row)
        .into()
}

fn view_ed2k_tab<'a>(
    p2p: &'a P2pSelectPopupState,
    ed2k_verify_md4: bool,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let visible = visible_ed2k_items(p2p);
    let selected_count = p2p.ed2k_items.iter().filter(|i| i.selected).count();
    let selected_bytes: u64 = p2p
        .ed2k_items
        .iter()
        .filter(|i| i.selected)
        .map(|i| i.file_size)
        .sum();
    let visible_all_selected = !visible.is_empty() && visible.iter().all(|i| i.selected);
    let has_input = !p2p.ed2k_input.trim().is_empty();

    let input_bar = row![
        icon(IconKind::Server, 14.0, palette.accent),
        wrap_input(
            text_input(
                "粘贴单条或多条 ed2k://|file|... 链接（支持直接粘贴整段含中文说明的网页文本）...",
                &p2p.ed2k_input
            )
            .id(p2p_ed2k_input_id())
            .on_input(Message::P2pEd2kInputChanged)
            .on_submit(Message::P2pParseEd2kInput)
            .font(FONT_REGULAR)
            .size(12)
            .padding([7, 10])
            .width(Length::Fill)
            .style(input_style(palette)),
            InputTarget::P2pEd2kInput,
        ),
        button(
            text("剪贴板导入")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([7, 11])
        .style(outline_btn_style(palette))
        .on_press(Message::P2pImportEd2kClipboard),
        button(
            text("解析提取")
                .size(12)
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::None),
        )
        .padding([7, 12])
        .style(primary_btn_style(palette))
        .on_press_maybe(has_input.then_some(Message::P2pParseEd2kInput)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut col = column![input_bar].spacing(9).height(Length::Fill);

    if let Some(err) = &p2p.ed2k_error {
        col = col.push(
            container(text(err).size(12).font(FONT_REGULAR).color(palette.err))
                .padding([6, 10])
                .width(Length::Fill)
                .style(panel_style(palette.err_soft, Some(palette.err), 6.0)),
        );
    }

    let filter_btn = |kind: P2pFileFilter, label: &'static str| {
        let count = p2p
            .ed2k_items
            .iter()
            .filter(|it| kind.matches_filename(&it.filename))
            .count();
        let active = p2p.filter == kind;
        let caption = if kind == P2pFileFilter::All || count > 0 {
            format!("{label} ({count})")
        } else {
            label.to_string()
        };
        button(
            text(caption)
                .size(12)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                .wrapping(text::Wrapping::None),
        )
        .padding([5, 10])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::P2pSetFilter(kind))
    };

    let filter_bar = row![
        filter_btn(P2pFileFilter::All, "全部"),
        filter_btn(P2pFileFilter::Video, "视频"),
        filter_btn(P2pFileFilter::Audio, "音频"),
        filter_btn(P2pFileFilter::ArchiveIso, "压缩包/镜像"),
        filter_btn(P2pFileFilter::DocImage, "文档/图片"),
        filter_btn(P2pFileFilter::Other, "其他"),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let toolbar = row![
        wrap_input(
            text_input("按文件名或 MD4 哈希过滤...", &p2p.search)
                .on_input(Message::P2pSearchChanged)
                .font(FONT_REGULAR)
                .size(12)
                .padding([6, 10])
                .width(Length::Fill)
                .style(input_style(palette)),
            InputTarget::P2pSearch,
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
            (!visible.is_empty()).then_some(Message::P2pSelectAllVisible(!visible_all_selected))
        ),
        button(
            text("反选")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([6, 10])
        .style(outline_btn_style(palette))
        .on_press_maybe((!visible.is_empty()).then_some(Message::P2pInvertSelection)),
        button(
            text("清空")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([6, 10])
        .style(danger_btn_style(palette))
        .on_press_maybe((!p2p.ed2k_items.is_empty()).then_some(Message::P2pClearEd2kItems)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let list_body: Element<'a, Message> = if visible.is_empty() {
        container(
            column![
                icon(IconKind::Server, 28.0, palette.text_muted),
                text(if p2p.ed2k_items.is_empty() {
                    "暂无待选 eD2k 电驴资源"
                } else {
                    "当前筛选条件下没有匹配的 eD2k 文件"
                })
                .size(14)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
                text("可粘贴单条或批量 ed2k://|file|... 链接（自动按 MD4 哈希去重并解析精确文件大小）")
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
        for item in visible {
            let (tag_txt, tag_fg, tag_bg) = category_badge_for_path(&item.filename, palette);
            let chunks = item.file_size.div_ceil(ED2K_CHUNK_SIZE).max(1);
            let short_md4 = &item.md4_hash[..item.md4_hash.len().min(12)];
            let mut sub_parts = vec![
                format!("MD4: {short_md4}…"),
                format!("{chunks} 分块 (9.28MB/块)"),
            ];
            if item.web_sources > 0 {
                sub_parts.push(format!("Web源 {}", item.web_sources));
            }
            if item.peer_sources > 0 {
                sub_parts.push(format!("节点源 {}", item.peer_sources));
            }
            if item.aich_hash.is_some() {
                sub_parts.push("AICH".to_string());
            }

            let item_id = item.id;
            let card = container(
                row![
                    checkbox(item.selected)
                        .on_toggle(move |_| Message::P2pToggleEd2kItem(item_id))
                        .size(16)
                        .style(checkbox_style(palette, false)),
                    column![
                        row![
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
                                text(&item.filename)
                                    .size(13)
                                    .font(FONT_MEDIUM)
                                    .color(palette.text_primary)
                                    .wrapping(text::Wrapping::None),
                            )
                            .width(Length::Fill)
                            .clip(true),
                            text(format_bytes(item.file_size))
                                .size(12)
                                .font(FONT_MONO_BOLD)
                                .color(palette.text_secondary)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(7)
                        .align_y(Alignment::Center),
                        text(sub_parts.join("  ·  "))
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
                    .on_press(Message::P2pDownloadSingleEd2k(item_id)),
                    button(icon(IconKind::Close, 12.0, palette.text_muted))
                        .padding([5, 7])
                        .style(subtle_btn_style(palette, false))
                        .on_press(Message::P2pRemoveEd2kItem(item_id)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([8, 11])
            .width(Length::Fill)
            .style(panel_style(
                if item.selected {
                    palette.bg_card
                } else {
                    palette.bg_input
                },
                Some(if item.selected {
                    palette.accent
                } else {
                    palette.hairline
                }),
                8.0,
            ));

            rows_col = rows_col.push(card);
        }

        scrollable(rows_col)
            .height(Length::Fill)
            .style(clean_scrollable_style(palette))
            .into()
    };

    let dir_row = build_save_dir_row(p2p, palette);

    let action_row = row![
        text(format!(
            "已选 {selected_count} 项 ({})  ·  MD4 分块校验: {}",
            format_bytes(selected_bytes),
            if ed2k_verify_md4 { "开启" } else { "关闭" }
        ))
        .size(11)
        .font(FONT_REGULAR)
        .color(palette.text_muted),
        space::horizontal(),
        button(text("关闭").size(13).font(FONT_REGULAR))
            .padding([7, 14])
            .style(outline_btn_style(palette))
            .on_press(Message::P2pClose),
        button(
            text(format!("加入暂停队列 ({selected_count})"))
                .size(13)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([7, 14])
        .style(outline_btn_style(palette))
        .on_press_maybe((selected_count > 0).then_some(Message::P2pSubmitEd2kSelected(false))),
        button(
            row![
                icon(IconKind::Downloading, 14.0, palette.cta_text),
                text(format!("下载选中 eD2k ({selected_count})"))
                    .size(13)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([7, 16])
        .style(primary_btn_style(palette))
        .on_press_maybe((selected_count > 0).then_some(Message::P2pSubmitEd2kSelected(true))),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    col.push(filter_bar)
        .push(toolbar)
        .push(
            container(list_body)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding([2, 0]),
        )
        .push(rule::horizontal(1).style(hairline_rule_style(palette)))
        .push(dir_row)
        .push(action_row)
        .into()
}

fn build_save_dir_row<'a>(
    p2p: &'a P2pSelectPopupState,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    row![
        text("保存到")
            .size(12)
            .font(FONT_MEDIUM)
            .color(palette.text_secondary)
            .wrapping(text::Wrapping::None),
        wrap_input(
            text_input("默认按规则目录保存", &p2p.save_dir)
                .on_input(Message::P2pSaveDirChanged)
                .font(FONT_MONO)
                .size(12)
                .padding([6, 10])
                .width(Length::Fill)
                .style(input_style(palette)),
            InputTarget::P2pSaveDir,
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
        .on_press(Message::P2pBrowseDir),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn split_display_path(path: &str) -> (String, String) {
    let normalized = path.replace('\\', "/");
    if let Some((dir, name)) = normalized.rsplit_once('/') {
        (name.to_string(), dir.to_string())
    } else {
        (normalized, String::new())
    }
}

fn category_badge_for_path(path: &str, palette: SiphonPalette) -> (&'static str, Color, Color) {
    match FileCategory::from_filename_and_mime(path, None) {
        FileCategory::Video => ("VIDEO", palette.accent, palette.accent_soft),
        FileCategory::Audio => ("AUDIO", palette.ok, palette.ok_soft),
        FileCategory::Archive => ("ZIP", palette.warn, palette.warn_soft),
        FileCategory::DiskImage => ("ISO", palette.warn, palette.warn_soft),
        FileCategory::Software => ("APP", palette.accent, palette.accent_soft),
        FileCategory::Document => ("DOC", palette.text_secondary, palette.bg_card),
        FileCategory::Image => ("IMG", palette.text_secondary, palette.bg_card),
        FileCategory::Other => ("FILE", palette.text_secondary, palette.bg_card),
    }
}
