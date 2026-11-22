use crate::icons::{IconKind, icon};
use crate::state::{DeleteConfirmState, Message};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, checkbox_style,
    danger_solid_btn_style, modal_card_style, outline_btn_style, panel_style, primary_btn_style,
    subtle_btn_style,
};
use iced::widget::{button, checkbox, column, container, mouse_area, row, space, text};
use iced::{Alignment, Element, Length};

pub fn view<'a>(dlg: &'a DeleteConfirmState, palette: SiphonPalette) -> Element<'a, Message> {
    let icon_badge = container(icon(IconKind::Trash, 16.0, palette.err))
        .center_x(Length::Fixed(32.0))
        .center_y(Length::Fixed(32.0))
        .style(panel_style(palette.err_soft, None, 8.0));

    let header = row![
        icon_badge,
        text("删除下载任务")
            .size(16)
            .font(FONT_SEMIBOLD)
            .color(palette.text_primary),
        space::horizontal(),
        button(icon(IconKind::Close, 14.0, palette.text_secondary))
            .padding([6, 8])
            .style(subtle_btn_style(palette, false))
            .on_press(Message::DeleteConfirmCancel),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let file_card = container(
        column![
            text(&dlg.filename)
                .size(14)
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::WordOrGlyph)
                .color(palette.text_primary),
            text(dlg.file_path.to_string_lossy().to_string())
                .size(12)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::WordOrGlyph)
                .color(palette.text_muted),
        ]
        .spacing(4),
    )
    .padding([12, 14])
    .width(Length::Fill)
    .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0));

    let delete_files_box = container(
        column![
            checkbox(dlg.delete_files)
                .label("同时删除本地文件")
                .on_toggle(Message::DeleteConfirmToggleFiles)
                .size(16)
                .text_size(14)
                .font(FONT_MEDIUM)
                .style(checkbox_style(palette, true)),
            text(if dlg.delete_files {
                "将从磁盘永久移除已下载文件及未完成的 .siphon.part 临时文件"
            } else {
                "仅从任务列表移除记录，保留磁盘上的本地文件"
            })
            .size(12)
            .font(FONT_REGULAR)
            .color(if dlg.delete_files {
                palette.err
            } else {
                palette.text_muted
            }),
        ]
        .spacing(6),
    )
    .padding([12, 14])
    .width(Length::Fill)
    .style(panel_style(
        if dlg.delete_files {
            palette.err_soft
        } else {
            palette.bg_card
        },
        Some(if dlg.delete_files {
            palette.err
        } else {
            palette.hairline
        }),
        8.0,
    ));

    let dont_ask = checkbox(dlg.dont_ask_again)
        .label("不再弹出此确认窗口（以后按当前选项直接删除）")
        .on_toggle(Message::DeleteConfirmToggleDontAsk)
        .size(15)
        .text_size(13)
        .font(FONT_REGULAR)
        .style(checkbox_style(palette, false));

    let confirm_btn = if dlg.delete_files {
        button(
            container(
                row![
                    icon(IconKind::Trash, 13.0, iced::Color::WHITE),
                    text("删除任务及文件")
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
        .style(danger_solid_btn_style(palette))
        .on_press(Message::DeleteConfirmAccept)
    } else {
        button(
            container(
                text("仅移除记录")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
            )
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .height(Length::Fixed(34.0))
        .padding([0, 16])
        .style(primary_btn_style(palette))
        .on_press(Message::DeleteConfirmAccept)
    };

    let footer = row![
        space::horizontal(),
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
        .padding([0, 16])
        .style(outline_btn_style(palette))
        .on_press(Message::DeleteConfirmCancel),
        confirm_btn,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let card = container(
        column![
            header,
            file_card,
            delete_files_box,
            dont_ask,
            space::vertical().height(2),
            footer,
        ]
        .spacing(14),
    )
    .width(Length::Fixed(460.0))
    .padding(22)
    .style(modal_card_style(palette));

    mouse_area(
        container(mouse_area(card).on_press(Message::ClearToast))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(panel_style(palette.bg_overlay, None, 0.0)),
    )
    .on_press(Message::DeleteConfirmCancel)
    .into()
}
