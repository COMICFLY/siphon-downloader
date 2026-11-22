use crate::icons::{IconKind, icon};
use crate::state::{CaptureAction, CapturePopupState, InputTarget, Message};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, checkbox_style,
    hairline_rule_style, input_style, outline_btn_style, panel_style, primary_btn_style,
    window_ctrl_btn_style,
};
use iced::widget::{
    button, checkbox, column, container, image, mouse_area, row, rule, space, text, text_input,
};
use iced::{Alignment, Color, Element, Length};
use siphon_core::{FileCategory, TaskSource, format_bytes};

pub fn capture_filename_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-capture-filename")
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

pub fn view<'a>(
    popup: &'a CapturePopupState,
    sys_icon: Option<&'a image::Handle>,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let source_label = match popup.source {
        TaskSource::Browser => "浏览器下载捕获",
        TaskSource::Clipboard => "剪贴板直链捕获",
        _ => "下载任务捕获",
    };

    let drag_brand = mouse_area(
        container(
            row![
                icon(IconKind::Brand, 17.0, palette.accent),
                text("Siphon")
                    .size(14)
                    .font(FONT_SEMIBOLD)
                    .color(palette.text_primary),
                container(
                    text(source_label)
                        .size(11)
                        .font(FONT_MEDIUM)
                        .color(palette.accent),
                )
                .padding([2, 8])
                .style(panel_style(palette.accent_soft, None, 5.0)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([0, 16])
        .align_y(Alignment::Center),
    )
    .on_press(Message::CaptureDragWindow);

    let close_btn = button(
        container(icon(IconKind::Close, 13.0, palette.text_secondary))
            .center_x(Length::Fixed(36.0))
            .center_y(Length::Fixed(30.0)),
    )
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(30.0))
    .padding(0)
    .style(window_ctrl_btn_style(palette, true))
    .on_press(Message::CaptureCancel);

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

    let category = FileCategory::from_filename_and_mime(&popup.filename, None);
    let icon_inner: Element<'a, Message> = if let Some(handle) = sys_icon {
        image(handle.clone())
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(image::FilterMethod::Linear)
            .into()
    } else {
        icon(
            IconKind::from_category(category),
            20.0,
            palette.text_secondary,
        )
        .into()
    };

    let file_icon_box = container(icon_inner)
        .center_x(Length::Fixed(44.0))
        .center_y(Length::Fixed(44.0))
        .style(panel_style(palette.bg_input, Some(palette.hairline), 10.0));

    let size_txt = match (popup.known_size, popup.probing) {
        (Some(bytes), _) => format_bytes(bytes),
        (None, true) => "正在探测大小...".to_string(),
        (None, false) => "未知大小".to_string(),
    };

    let mut badges_row = row![meta_pill(size_txt, palette.text_primary, palette.bg_input, palette)]
        .spacing(6)
        .align_y(Alignment::Center);

    match popup.supports_range {
        Some(true) => {
            badges_row = badges_row.push(meta_pill(
                "支持断点续传".to_string(),
                palette.ok,
                palette.ok_soft,
                palette,
            ));
        }
        Some(false) => {
            badges_row = badges_row.push(meta_pill(
                "单线程流".to_string(),
                palette.warn,
                palette.warn_soft,
                palette,
            ));
        }
        None if popup.probing => {
            badges_row = badges_row.push(meta_pill(
                "探测源站中".to_string(),
                palette.text_muted,
                palette.bg_input,
                palette,
            ));
        }
        None => {}
    }

    if let Some(rule_name) = &popup.matched_rule_name {
        badges_row = badges_row.push(meta_pill(
            format!("规则: {rule_name}"),
            palette.accent,
            palette.accent_soft,
            palette,
        ));
    }

    let has_auth_context =
        popup.headers.contains_key("Cookie") || popup.headers.contains_key("Referer");
    if has_auth_context {
        badges_row = badges_row.push(meta_pill(
            "已携带浏览器凭证".to_string(),
            palette.text_secondary,
            palette.bg_input,
            palette,
        ));
    }

    let summary_card = container(
        row![
            file_icon_box,
            column![
                container(badges_row).width(Length::Fill).clip(true),
                container(
                    text(&popup.url)
                        .size(12)
                        .font(FONT_MONO)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_muted),
                )
                .width(Length::Fill)
                .clip(true),
            ]
            .spacing(6)
            .width(Length::Fill),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding([12, 14])
    .width(Length::Fill)
    .clip(true)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0));

    let filename_block = column![
        row![
            text("文件名")
                .size(12)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
            space::horizontal(),
            text(if let Some(err) = &popup.probe_error {
                format!("提示: {err}")
            } else {
                format!("并发 {} 线程", popup.connections)
            })
            .size(12)
            .font(FONT_MONO)
            .color(if popup.probe_error.is_some() {
                palette.warn
            } else {
                palette.text_muted
            }),
        ]
        .align_y(Alignment::Center),
        wrap_input(
            text_input("输入保存文件名", &popup.filename)
                .id(capture_filename_id())
                .on_input(Message::CaptureFilenameChanged)
                .on_submit(Message::CaptureSubmit(CaptureAction::StartWithDialog))
                .font(FONT_REGULAR)
                .size(13)
                .padding([8, 12])
                .width(Length::Fill)
                .style(input_style(palette)),
            InputTarget::CaptureFilename
        ),
    ]
    .spacing(5);

    let dir_block = column![
        text("保存至目录")
            .size(12)
            .font(FONT_MEDIUM)
            .color(palette.text_secondary),
        row![
            wrap_input(
                text_input("选择保存路径", &popup.save_dir)
                    .on_input(Message::CaptureSaveDirChanged)
                    .font(FONT_MONO)
                    .size(13)
                    .padding([8, 12])
                    .width(Length::Fill)
                    .style(input_style(palette)),
                InputTarget::CaptureSaveDir
            ),
            button(
                row![
                    icon(IconKind::Folder, 14.0, palette.text_secondary),
                    text("浏览")
                        .size(13)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([8, 14])
            .style(outline_btn_style(palette))
            .on_press(Message::CaptureBrowseDir),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(5);

    let dont_ask_label = if popup.source == TaskSource::Browser {
        "以后捕获浏览器下载自动按规则静默开下，不再弹出此窗口"
    } else {
        "以后复制下载链接仅在主界面顶部提示，不再弹出独立窗口"
    };

    let dont_ask = checkbox(popup.dont_ask_again)
        .label(dont_ask_label)
        .on_toggle(Message::CaptureToggleDontAsk)
        .size(15)
        .text_size(12)
        .font(FONT_REGULAR)
        .style(checkbox_style(palette, false));

    let can_submit = !popup.url.trim().is_empty() && !popup.filename.trim().is_empty();

    let footer = row![
        button(
            container(
                text("取消")
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
        .on_press(Message::CaptureCancel),
        space::horizontal(),
        button(
            container(
                row![
                    icon(IconKind::Paused, 13.0, palette.text_secondary),
                    text("稍后下载")
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
        .padding([0, 13])
        .style(outline_btn_style(palette))
        .on_press_maybe(can_submit.then_some(Message::CaptureSubmit(CaptureAction::Later))),
        button(
            container(
                row![
                    icon(IconKind::Bolt, 13.0, palette.text_secondary),
                    text("后台下载")
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
        .padding([0, 13])
        .style(outline_btn_style(palette))
        .on_press_maybe(can_submit.then_some(Message::CaptureSubmit(CaptureAction::Background))),
        button(
            container(
                row![
                    icon(IconKind::Downloading, 14.0, palette.cta_text),
                    text("开始下载")
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
        .on_press_maybe(
            can_submit.then_some(Message::CaptureSubmit(CaptureAction::StartWithDialog)),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let body = column![
        summary_card,
        filename_block,
        dir_block,
        dont_ask,
        space::vertical(),
        footer,
    ]
    .spacing(12)
    .padding([16, 20])
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

fn meta_pill<'a>(
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
    .padding([3, 8])
    .style(panel_style(bg, Some(palette.hairline), 5.0))
    .into()
}
