use crate::i18n::Strings;
use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, hairline_rule_style,
    outline_btn_style, panel_style, primary_btn_style, seamless_input_style, subtle_btn_style,
    window_ctrl_btn_style,
};
use iced::widget::{button, container, mouse_area, row, rule, space, text, text_input};
use iced::{Alignment, Element, Length};

pub fn quick_bar_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-quick-bar")
}

pub fn view<'a>(
    quick_input: &'a str,
    palette: SiphonPalette,
    strings: Strings,
) -> Element<'a, Message> {
    // Exact 240px left brand bay — aligns pixel-for-pixel with the 240px sidebar seam below
    let brand_bay = mouse_area(
        container(
            row![
                icon(IconKind::Brand, 18.0, palette.text_primary),
                text("Siphon")
                    .size(16)
                    .font(FONT_SEMIBOLD)
                    .color(palette.text_primary),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .width(Length::Fixed(240.0))
        .height(Length::Fill)
        .padding([0, 20])
        .align_y(Alignment::Center),
    )
    .on_press(Message::DragWindow)
    .on_double_click(Message::ToggleMaximizeWindow);

    let is_url = quick_input.trim().starts_with("http://")
        || quick_input.trim().starts_with("https://");

    let input_field = mouse_area(
        text_input(strings.quick_bar_placeholder(), quick_input)
            .id(quick_bar_id())
            .on_input(Message::QuickBarChanged)
            .on_submit(Message::QuickBarSubmit)
            .font(FONT_REGULAR)
            .size(14)
            .padding([0, 4])
            .width(Length::Fill)
            .style(seamless_input_style(palette)),
    )
    .on_release(Message::FocusInputTarget(InputTarget::QuickBar))
    .on_right_press(Message::OpenInputContextMenu(InputTarget::QuickBar));

    let trailing_indicator: Element<'a, Message> = if is_url {
        button(
            row![
                icon(IconKind::Bolt, 12.0, palette.cta_text),
                text(if strings.lang == siphon_core::Language::ZhCn {
                    "下载"
                } else {
                    "Go"
                })
                .size(12)
                .font(FONT_MEDIUM),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([3, 10])
        .style(primary_btn_style(palette))
        .on_press(Message::QuickBarSubmit)
        .into()
    } else {
        container(
            text("Ctrl K")
                .size(11)
                .font(FONT_MONO)
                .color(palette.text_muted),
        )
        .padding([2, 6])
        .style(panel_style(palette.bg_raised, Some(palette.hairline), 5.0))
        .into()
    };

    let search_shell = container(
        row![
            icon(
                if is_url {
                    IconKind::Link
                } else {
                    IconKind::Search
                },
                14.0,
                if is_url {
                    palette.accent
                } else {
                    palette.text_muted
                }
            ),
            input_field,
            trailing_indicator,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .height(Length::Fixed(34.0))
    .width(Length::Fixed(360.0))
    .padding([0, 10])
    .align_y(Alignment::Center)
    .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0));

    let new_btn = button(
        container(
            row![
                icon(IconKind::Plus, 14.0, palette.cta_text),
                text(strings.new_task())
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
    .height(Length::Fixed(34.0))
    .padding([0, 14])
    .style(primary_btn_style(palette))
    .on_press(Message::OpenNewTaskModal(None));

    let sniff_btn = button(
        container(
            row![
                icon(IconKind::Search, 13.0, palette.accent),
                text(if strings.lang == siphon_core::Language::ZhCn {
                    "资源嗅探"
                } else {
                    "Sniff"
                })
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
    .padding([0, 12])
    .style(outline_btn_style(palette))
    .on_press(Message::OpenEmptySniffPopup);

    // Only drag_spacer uses Length::Fill so right-side window controls NEVER stretch in fullscreen!
    let drag_spacer = mouse_area(container(space()).width(Length::Fill).height(Length::Fill))
        .on_press(Message::DragWindow)
        .on_double_click(Message::ToggleMaximizeWindow);

    let theme_btn = button(
        container(icon(
            if palette.is_dark {
                IconKind::Sun
            } else {
                IconKind::Moon
            },
            15.0,
            palette.text_secondary,
        ))
        .center_x(Length::Fixed(32.0))
        .center_y(Length::Fixed(32.0)),
    )
    .width(Length::Fixed(32.0))
    .height(Length::Fixed(32.0))
    .padding(0)
    .style(subtle_btn_style(palette, false))
    .on_press(Message::CycleTheme);

    let ctrl_btn = |kind: IconKind, size: f32, is_close: bool, msg: Message| {
        button(
            container(icon(kind, size, palette.text_secondary))
                .center_x(Length::Fixed(36.0))
                .center_y(Length::Fixed(32.0)),
        )
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(32.0))
        .padding(0)
        .style(window_ctrl_btn_style(palette, is_close))
        .on_press(msg)
    };

    let right_controls = row![
        theme_btn,
        container(rule::vertical(1).style(hairline_rule_style(palette)))
            .width(Length::Fixed(9.0))
            .height(Length::Fixed(16.0))
            .center_x(Length::Fixed(9.0)),
        ctrl_btn(IconKind::Minimize, 13.0, false, Message::MinimizeWindow),
        ctrl_btn(
            IconKind::Maximize,
            12.0,
            false,
            Message::ToggleMaximizeWindow
        ),
        ctrl_btn(IconKind::Close, 13.0, true, Message::CloseWindow),
    ]
    .width(Length::Shrink)
    .spacing(2)
    .align_y(Alignment::Center);

    let main_header_bay = row![search_shell, new_btn, sniff_btn, drag_spacer, right_controls]
        .spacing(12)
        .padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            bottom: 0.0,
            left: 28.0,
        })
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Fill);

    let bar = row![
        brand_bay,
        rule::vertical(1).style(hairline_rule_style(palette)),
        main_header_bay,
    ]
    .width(Length::Fill)
    .height(Length::Fixed(52.0));

    container(bar)
        .width(Length::Fill)
        .height(Length::Fixed(52.0))
        .style(panel_style(palette.bg_chrome, None, 0.0))
        .into()
}
