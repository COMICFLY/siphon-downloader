use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message, NewTaskModalState};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, input_style,
    modal_card_style, outline_btn_style, panel_style, primary_btn_style, subtle_btn_style,
};
use iced::widget::{button, column, container, mouse_area, row, slider, space, text, text_input};
use iced::{Alignment, Element, Length};
use siphon_core::{expand_batch_input, format_bytes};

pub fn modal_url_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-modal-url")
}

pub fn modal_batch_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-modal-batch-urls")
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

pub fn view<'a>(modal: &'a NewTaskModalState, palette: SiphonPalette) -> Element<'a, Message> {
    let mode_switch = container(
        row![
            button(
                text("单链接 / 种子")
                    .size(12)
                    .font(if !modal.batch_mode {
                        FONT_MEDIUM
                    } else {
                        FONT_REGULAR
                    }),
            )
            .padding([5, 12])
            .style(subtle_btn_style(palette, !modal.batch_mode))
            .on_press(Message::ModalToggleBatchMode(false)),
            button(
                row![
                    icon(
                        IconKind::Layers,
                        12.0,
                        if modal.batch_mode {
                            palette.text_primary
                        } else {
                            palette.text_muted
                        },
                    ),
                    text("批量通配符队列")
                        .size(12)
                        .font(if modal.batch_mode {
                            FONT_MEDIUM
                        } else {
                            FONT_REGULAR
                        }),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([5, 12])
            .style(subtle_btn_style(palette, modal.batch_mode))
            .on_press(Message::ModalToggleBatchMode(true)),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    )
    .padding(2)
    .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0));

    let header = row![
        text("新建下载任务")
            .size(16)
            .font(FONT_SEMIBOLD)
            .color(palette.text_primary),
        space::horizontal().width(12),
        mode_switch,
        space::horizontal(),
        button(icon(IconKind::Close, 14.0, palette.text_secondary))
            .padding([6, 8])
            .style(subtle_btn_style(palette, false))
            .on_press(Message::CloseNewTaskModal),
    ]
    .align_y(Alignment::Center);

    let dir_field = column![
        text("保存目录")
            .size(13)
            .font(FONT_MEDIUM)
            .color(palette.text_secondary),
        row![
            wrap_input(
                text_input("选择保存路径", &modal.save_dir)
                    .on_input(Message::ModalSaveDirChanged)
                    .font(FONT_MONO)
                    .size(13)
                    .padding([8, 12])
                    .width(Length::Fill)
                    .style(input_style(palette)),
                InputTarget::ModalSaveDir
            ),
            button(
                row![
                    icon(IconKind::Folder, 14.0, palette.text_secondary),
                    text("浏览").size(13).font(FONT_REGULAR),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([8, 14])
            .style(outline_btn_style(palette))
            .on_press(Message::ModalBrowseDir),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(6);

    let tuning_row = row![
        column![
            row![
                text("并发线程数")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                space::horizontal(),
                text(format!("{} 线程", modal.connections))
                    .size(13)
                    .font(FONT_MONO)
                    .color(palette.text_primary),
            ],
            slider(
                1.0_f32..=32.0_f32,
                modal.connections as f32,
                Message::ModalConnectionsChanged
            )
            .step(1.0_f32),
        ]
        .spacing(8)
        .width(Length::FillPortion(3)),
        column![
            text("单任务限速 (KB/s)")
                .size(13)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
            wrap_input(
                text_input("0 为不限速", &modal.speed_limit_kbps)
                    .on_input(Message::ModalSpeedLimitChanged)
                    .font(FONT_MONO)
                    .size(13)
                    .padding([8, 10])
                    .style(input_style(palette)),
                InputTarget::ModalSpeedLimit
            ),
        ]
        .spacing(6)
        .width(Length::FillPortion(2)),
    ]
    .spacing(16);

    let mut form = column![header].spacing(14);

    if modal.batch_mode {
        let expanded = expand_batch_input(&modal.batch_text, &modal.batch_filter);
        let count = expanded.urls.len();

        let batch_input_section = column![
            row![
                text("通配符模板或批量链接")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                space::horizontal(),
                button(text("粘贴剪贴板多行链接").size(12).font(FONT_REGULAR))
                    .padding([4, 10])
                    .style(outline_btn_style(palette))
                    .on_press(Message::ModalBatchPasteClipboard),
            ]
            .align_y(Alignment::Center),
            wrap_input(
                text_input(
                    "例如 https://example.com/file-[001-020].zip 或用空格/粘贴导入多行链接",
                    &modal.batch_text,
                )
                .id(modal_batch_id())
                .on_input(Message::ModalBatchTextChanged)
                .font(FONT_MONO)
                .size(13)
                .padding([9, 12])
                .width(Length::Fill)
                .style(input_style(palette)),
                InputTarget::ModalBatchUrls,
            ),
            text("支持 [001-100]、[1-20:2] 步长、[a-z] 字母通配符与多通配符笛卡尔展开，同时支持 HTTP / HLS (.m3u8) / Magnet / eD2k")
                .size(11)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
        ]
        .spacing(6);

        let filter_row = row![
            text("筛选关键词 / 后缀（可选）")
                .size(12)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
            wrap_input(
                text_input("例如 .mp4 或 1080p", &modal.batch_filter)
                    .on_input(Message::ModalBatchFilterChanged)
                    .font(FONT_MONO)
                    .size(12)
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(input_style(palette)),
                InputTarget::ModalBatchFilter,
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let mut preview_col = column![
            row![
                text(format!("实时展开预览 · 共 {count} 个任务"))
                    .size(12)
                    .font(FONT_MEDIUM)
                    .color(if count > 0 {
                        palette.accent
                    } else {
                        palette.text_muted
                    }),
                space::horizontal(),
                if expanded.truncated {
                    text("已达单次上限 1000 项")
                        .size(11)
                        .font(FONT_MONO)
                        .color(palette.warn)
                } else {
                    text("自动去重")
                        .size(11)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted)
                },
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(4);

        if count == 0 {
            preview_col = preview_col.push(
                text("输入通配符 URL（如 https://cdn.example.com/ep-[01-10].mp4）或粘贴多行链接即可实时预览")
                    .size(11)
                    .font(FONT_REGULAR)
                    .color(palette.text_muted),
            );
        } else {
            for (idx, u) in expanded.urls.iter().take(5).enumerate() {
                let shown = if u.len() > 74 {
                    format!("{}...", &u[..71])
                } else {
                    u.clone()
                };
                preview_col = preview_col.push(
                    text(format!("{:>3}. {shown}", idx + 1))
                        .size(11)
                        .font(FONT_MONO)
                        .color(palette.text_secondary),
                );
            }
            if count > 5 {
                preview_col = preview_col.push(
                    text(format!("    ... 以及其余 {} 项链接", count - 5))
                        .size(11)
                        .font(FONT_MONO)
                        .color(palette.text_muted),
                );
            }
        }

        let preview_box = container(preview_col)
            .padding([10, 12])
            .width(Length::Fill)
            .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0));

        let can_batch = count > 0;
        let footer = row![
            button(text("取消").size(13).font(FONT_REGULAR))
                .padding([8, 16])
                .style(outline_btn_style(palette))
                .on_press(Message::CloseNewTaskModal),
            space::horizontal(),
            button(text("全部加入暂停队列").size(13).font(FONT_REGULAR))
                .padding([8, 14])
                .style(outline_btn_style(palette))
                .on_press_maybe(can_batch.then_some(Message::ModalBatchSubmit(false))),
            button(
                row![
                    icon(IconKind::Downloading, 14.0, palette.cta_text),
                    text(format!("批量开始下载 ({count})"))
                        .size(13)
                        .font(FONT_MEDIUM),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([8, 18])
            .style(primary_btn_style(palette))
            .on_press_maybe(can_batch.then_some(Message::ModalBatchSubmit(true))),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        form = form
            .push(batch_input_section)
            .push(filter_row)
            .push(preview_box)
            .push(dir_field)
            .push(tuning_row)
            .push(space::vertical().height(4))
            .push(footer);
    } else {
        let probe_btn = button(
            text(if modal.probing { "探测中..." } else { "探测" })
                .size(13)
                .font(FONT_MEDIUM),
        )
        .padding([8, 14])
        .style(outline_btn_style(palette))
        .on_press_maybe(
            (!modal.probing && !modal.url.trim().is_empty()).then_some(Message::ModalProbeUrl),
        );

        let torrent_btn = button(text("导入 .torrent").size(12).font(FONT_REGULAR))
            .padding([8, 10])
            .style(outline_btn_style(palette))
            .on_press(Message::ModalPickTorrentFile);

        let url_row = column![
            text("下载链接（支持 HTTP/HTTPS · HLS .m3u8 · Magnet 磁力 · 本地 .torrent · eD2k 电驴）")
                .size(12)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
            row![
                wrap_input(
                    text_input(
                        "https://... · .m3u8 · magnet:?xt=... · ed2k://|file|...",
                        &modal.url
                    )
                    .id(modal_url_id())
                    .on_input(Message::ModalUrlChanged)
                    .on_submit(Message::ModalProbeUrl)
                    .font(FONT_REGULAR)
                    .size(14)
                    .padding([8, 12])
                    .width(Length::Fill)
                    .style(input_style(palette)),
                    InputTarget::ModalUrl
                ),
                probe_btn,
                torrent_btn,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(6);

        form = form.push(url_row);

        if let Some(err) = &modal.probe_error {
            form = form.push(
                container(text(err).size(13).font(FONT_REGULAR).color(palette.err))
                    .padding([10, 12])
                    .width(Length::Fill)
                    .style(panel_style(palette.err_soft, None, 8.0)),
            );
        }

        if let Some(preview) = &modal.preview {
            let size_txt = preview
                .metadata
                .content_length
                .map(format_bytes)
                .unwrap_or_else(|| "未知大小".into());
            let range_txt = if preview.metadata.supports_range {
                "支持多线程断点续传"
            } else {
                "单线程流式传输"
            };
            let mut meta_bar = row![
                meta_tag(size_txt, palette.text_primary, palette),
                meta_tag(
                    range_txt.to_string(),
                    if preview.metadata.supports_range {
                        palette.ok
                    } else {
                        palette.warn
                    },
                    palette,
                ),
            ]
            .spacing(8);

            if let Some(note) = &preview.metadata.plugin_note {
                meta_bar = meta_bar.push(meta_tag(note.clone(), palette.accent, palette));
            }
            if let Some(rule_name) = &preview.matched_rule_name {
                meta_bar = meta_bar.push(meta_tag(
                    format!("命中规则: {rule_name}"),
                    palette.accent,
                    palette,
                ));
            }
            form = form.push(meta_bar);
        }

        let filename_field = column![
            text("文件名")
                .size(13)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
            wrap_input(
                text_input("留空则自动识别", &modal.filename)
                    .on_input(Message::ModalFilenameChanged)
                    .font(FONT_REGULAR)
                    .size(14)
                    .padding([8, 12])
                    .style(input_style(palette)),
                InputTarget::ModalFilename
            ),
        ]
        .spacing(6);

        let hash_field = column![
            text("SHA-256 校验值（可选）")
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
            wrap_input(
                text_input("留空跳过下载后哈希校验", &modal.sha256)
                    .on_input(Message::ModalSha256Changed)
                    .font(FONT_MONO)
                    .size(12)
                    .padding([8, 10])
                    .style(input_style(palette)),
                InputTarget::ModalSha256
            ),
        ]
        .spacing(5);

        let can_submit = !modal.url.trim().is_empty();
        let footer = row![
            button(text("取消").size(13).font(FONT_REGULAR))
                .padding([8, 16])
                .style(outline_btn_style(palette))
                .on_press(Message::CloseNewTaskModal),
            space::horizontal(),
            button(text("添加到暂停").size(13).font(FONT_REGULAR))
                .padding([8, 14])
                .style(outline_btn_style(palette))
                .on_press_maybe(can_submit.then_some(Message::ModalSubmit(false))),
            button(
                row![
                    icon(IconKind::Downloading, 14.0, palette.cta_text),
                    text("开始下载").size(13).font(FONT_MEDIUM),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([8, 18])
            .style(primary_btn_style(palette))
            .on_press_maybe(can_submit.then_some(Message::ModalSubmit(true))),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        form = form
            .push(filename_field)
            .push(dir_field)
            .push(tuning_row)
            .push(hash_field)
            .push(space::vertical().height(4))
            .push(footer);
    }

    let card = container(form)
        .width(Length::Fixed(610.0))
        .padding(24)
        .style(modal_card_style(palette));

    mouse_area(
        container(mouse_area(card).on_press(Message::ClearToast))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(panel_style(palette.bg_overlay, None, 0.0)),
    )
    .on_press(Message::CloseNewTaskModal)
    .into()
}

fn meta_tag<'a>(
    label: String,
    fg: iced::Color,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    container(text(label).size(12).font(FONT_MONO).color(fg))
        .padding([4, 10])
        .style(panel_style(palette.bg_input, Some(palette.hairline), 6.0))
        .into()
}
