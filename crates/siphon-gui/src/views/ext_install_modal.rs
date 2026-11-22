use crate::icons::{IconKind, icon};
use crate::state::{ExtInstallModalState, Message};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, hairline_rule_style,
    modal_card_style, outline_btn_style, panel_style, primary_btn_style, subtle_btn_style,
};
use iced::widget::{button, column, container, mouse_area, row, rule, space, text};
use iced::{Alignment, Element, Length};

pub fn view<'a>(
    modal: &'a ExtInstallModalState,
    browser_ext_connected: bool,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let status_pill = if browser_ext_connected {
        container(
            row![
                container(space())
                    .width(Length::Fixed(7.0))
                    .height(Length::Fixed(7.0))
                    .style(panel_style(palette.ok, None, 4.0)),
                text("已检测到插件连接")
                    .size(12)
                    .font(FONT_MEDIUM)
                    .color(palette.ok),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([3, 10])
        .style(panel_style(palette.ok_soft, Some(palette.hairline), 6.0))
    } else {
        container(
            row![
                container(space())
                    .width(Length::Fixed(7.0))
                    .height(Length::Fixed(7.0))
                    .style(panel_style(palette.text_muted, None, 4.0)),
                text("等待浏览器加载")
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([3, 10])
        .style(panel_style(palette.bg_input, Some(palette.hairline), 6.0))
    };

    let header = row![
        row![
            icon(IconKind::Plugins, 18.0, palette.accent),
            text("安装浏览器下载捕获插件")
                .size(16)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        space::horizontal(),
        status_pill,
        button(icon(IconKind::Close, 14.0, palette.text_secondary))
            .padding([5, 7])
            .style(subtle_btn_style(palette, false))
            .on_press(Message::CloseExtInstallModal),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    // Step 1: Browser picker
    let mut browser_chips = row![].spacing(8).align_y(Alignment::Center);
    if modal.browsers.is_empty() {
        browser_chips = browser_chips.push(
            text("未自动检测到 Edge / Chrome，您可复制下方扩展目录路径手动加载")
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
        );
    } else {
        for b in &modal.browsers {
            let selected = modal.selected_browser_id == b.id;
            let label = if b.is_default {
                format!("{} (默认)", b.name)
            } else {
                b.name.clone()
            };
            let id = b.id.clone();
            browser_chips = browser_chips.push(
                button(
                    text(label)
                        .size(13)
                        .font(if selected { FONT_MEDIUM } else { FONT_REGULAR })
                        .color(if selected {
                            palette.accent
                        } else {
                            palette.text_primary
                        }),
                )
                .padding([7, 13])
                .style(subtle_btn_style(palette, selected))
                .on_press(Message::ExtInstallSelectBrowser(id)),
            );
        }
    }

    let browser_section = column![
        text("1. 选择目标浏览器（已自动识别系统默认浏览器）")
            .size(13)
            .font(FONT_MEDIUM)
            .color(palette.text_secondary),
        browser_chips,
    ]
    .spacing(8);

    // Step 2: Deployed extension path + copy button
    let path_str = modal.ext_dir.to_string_lossy().to_string();
    let copy_label = if modal.path_copied {
        "已复制路径"
    } else {
        "复制路径"
    };
    let path_section = column![
        row![
            text("2. 扩展文件与图标已自动部署（含 Native Host 注册）")
                .size(13)
                .font(FONT_MEDIUM)
                .color(palette.text_secondary),
            space::horizontal(),
            text(if modal.deployed { "已就绪" } else { "部署失败" })
                .size(12)
                .font(FONT_MONO)
                .color(if modal.deployed {
                    palette.ok
                } else {
                    palette.err
                }),
        ]
        .align_y(Alignment::Center),
        row![
            container(
                text(path_str)
                    .size(12)
                    .font(FONT_MONO)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_primary),
            )
            .width(Length::Fill)
            .clip(true)
            .padding([8, 12])
            .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
            button(
                text(copy_label)
                    .size(12)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
            )
            .padding([8, 12])
            .style(outline_btn_style(palette))
            .on_press(Message::ExtInstallCopyPath),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(8);

    // Step 3: Clean visual steps
    let guide_box = container(
        column![
            text("3. 三步完成极速加载（支持直接拖放安装）")
                .size(13)
                .font(FONT_MEDIUM)
                .color(palette.text_primary),
            step_line(
                "01",
                "点击下方「一键打开扩展页并高亮文件夹」，自动拉起浏览器扩展页并复制路径",
                palette
            ),
            step_line(
                "02",
                "在打开的浏览器扩展管理页中打开右上角「开发者模式」开关",
                palette
            ),
            step_line(
                "03",
                "直接将高亮选中的 browser-extension 文件夹拖入浏览器页面（或点击「加载解压缩的扩展」后 Ctrl+V 回车）",
                palette
            ),
        ]
        .spacing(8),
    )
    .padding([12, 14])
    .width(Length::Fill)
    .style(panel_style(palette.bg_input, Some(palette.hairline), 10.0));

    let mut content = column![
        header,
        rule::horizontal(1).style(hairline_rule_style(palette)),
        browser_section,
        path_section,
        guide_box,
    ]
    .spacing(14);

    if let Some(err) = &modal.error_msg {
        content = content.push(
            container(
                text(err)
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.err),
            )
            .padding([8, 12])
            .width(Length::Fill)
            .style(panel_style(palette.err_soft, None, 8.0)),
        );
    }

    if browser_ext_connected {
        content = content.push(
            container(
                text("已实时检测到浏览器扩展成功接入 Siphon！您现在可以直接在浏览器触发下载测试捕获弹窗。")
                    .size(12)
                    .font(FONT_MEDIUM)
                    .color(palette.ok),
            )
            .padding([9, 12])
            .width(Length::Fill)
            .style(panel_style(palette.ok_soft, Some(palette.hairline), 8.0)),
        );
    } else if modal.browser_launched {
        content = content.push(
            container(
                text("已拉起浏览器扩展页并高亮文件夹（路径已复制）— 加载完成后本窗口上方状态会自动变为绿色「已检测到插件连接」。")
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.accent),
            )
            .padding([9, 12])
            .width(Length::Fill)
            .style(panel_style(palette.accent_soft, Some(palette.hairline), 8.0)),
        );
    }

    let selected_browser_name = modal
        .browsers
        .iter()
        .find(|b| b.id == modal.selected_browser_id)
        .map(|b| b.name.as_str())
        .unwrap_or("浏览器");

    let launch_label = format!("一键打开 {selected_browser_name} 扩展页并高亮文件夹");

    let footer = row![
        button(
            text("关闭")
                .size(13)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([8, 16])
        .style(outline_btn_style(palette))
        .on_press(Message::CloseExtInstallModal),
        space::horizontal(),
        button(
            row![
                icon(IconKind::Archive, 14.0, palette.text_secondary),
                text("导出 .zip")
                    .size(13)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([8, 13])
        .style(outline_btn_style(palette))
        .on_press(Message::ExportBrowserExtensionZip),
        button(
            row![
                icon(IconKind::Folder, 14.0, palette.text_secondary),
                text("打开目录")
                    .size(13)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([8, 13])
        .style(outline_btn_style(palette))
        .on_press(Message::ExtInstallRevealFolder),
        button(
            row![
                icon(IconKind::Bolt, 13.0, palette.cta_text),
                text(launch_label)
                    .size(13)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([8, 16])
        .style(primary_btn_style(palette))
        .on_press(Message::ExtInstallLaunchSelectedBrowser),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    content = content.push(footer);

    let card = container(mouse_area(content).on_press(Message::ClearToast))
        .width(Length::Fixed(610.0))
        .padding(22)
        .style(modal_card_style(palette));

    mouse_area(
        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(panel_style(palette.bg_overlay, None, 0.0)),
    )
    .on_press(Message::CloseExtInstallModal)
    .into()
}

fn step_line<'a>(num: &'static str, body: &'static str, palette: SiphonPalette) -> Element<'a, Message> {
    row![
        container(
            text(num)
                .size(11)
                .font(FONT_MONO)
                .color(palette.accent),
        )
        .padding([2, 6])
        .style(panel_style(palette.accent_soft, None, 5.0)),
        text(body)
            .size(12)
            .font(FONT_REGULAR)
            .color(palette.text_secondary),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}
