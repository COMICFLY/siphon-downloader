use crate::icons::{IconKind, icon};
use crate::state::Message;
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, clean_scrollable_style,
    outline_btn_style, panel_style, primary_btn_style,
};
use iced::widget::{button, column, container, row, scrollable, space, text, toggler};
use iced::{Alignment, Element, Length};
use siphon_core::ExternalPlugin;
use std::path::Path;

pub fn view<'a>(
    plugins: &'a [ExternalPlugin],
    plugins_dir: &Path,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let header = row![
        column![
            text("插件扩展")
                .size(22)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            text("一键安装浏览器下载捕获扩展，或通过 plugin.toml 与 JSON stdio 协议扩展站点解析与后处理")
                .size(14)
                .font(FONT_REGULAR)
                .color(palette.text_secondary),
        ]
        .spacing(4),
        space::horizontal(),
        button(
            row![
                icon(IconKind::Plus, 13.0, palette.cta_text),
                text("一键导入插件").size(13).font(FONT_MEDIUM),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([7, 14])
        .style(primary_btn_style(palette))
        .on_press(Message::InstallExternalPlugin),
        button(
            row![
                icon(IconKind::Folder, 14.0, palette.text_secondary),
                text("打开目录").size(13).font(FONT_REGULAR),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([7, 14])
        .style(outline_btn_style(palette))
        .on_press(Message::OpenPluginsFolder),
        button(
            row![
                icon(IconKind::Refresh, 14.0, palette.text_secondary),
                text("重新扫描").size(13).font(FONT_REGULAR),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([7, 14])
        .style(outline_btn_style(palette))
        .on_press(Message::ReloadPlugins),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let browser_ext_card = container(
        row![
            column![
                row![
                    text("Chrome / Edge 浏览器下载捕获插件")
                        .size(14)
                        .font(FONT_SEMIBOLD)
                        .color(palette.text_primary),
                    container(
                        text("Built-in MV3")
                            .size(11)
                            .font(FONT_MONO)
                            .color(palette.accent)
                    )
                    .padding([2, 8])
                    .style(panel_style(palette.accent_soft, None, 5.0)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                text("自动接管浏览器下载并携带 Cookie / Referer 弹出独立捕获小窗。点击右侧「一键安装浏览器插件」将自动部署扩展、复制路径并拉起浏览器扩展页。")
                    .size(13)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary),
            ]
            .spacing(5)
            .width(Length::Fill),
            row![
                button(
                    row![
                        icon(IconKind::Archive, 14.0, palette.text_secondary),
                        text("导出 .zip").size(13).font(FONT_REGULAR),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 12])
                .style(outline_btn_style(palette))
                .on_press(Message::ExportBrowserExtensionZip),
                button(
                    row![
                        icon(IconKind::Folder, 14.0, palette.text_secondary),
                        text("扩展目录").size(13).font(FONT_REGULAR),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 12])
                .style(outline_btn_style(palette))
                .on_press(Message::OpenBrowserExtensionFolder),
                button(
                    row![
                        icon(IconKind::Plugins, 14.0, palette.cta_text),
                        text("浏览器插件安装向导").size(13).font(FONT_MEDIUM),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 14])
                .style(primary_btn_style(palette))
                .on_press(Message::InstallBrowserExtension),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([16, 20])
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    let dir_info = container(
        text(format!("用户插件目录  {}", plugins_dir.display()))
            .size(12)
            .font(FONT_MONO)
            .color(palette.text_secondary),
    )
    .padding([10, 14])
    .width(Length::Fill)
    .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0));

    let mut list_col = column![].spacing(10);
    if plugins.is_empty() {
        list_col = list_col.push(
            container(
                text("尚未发现外部插件。打开插件目录并放入包含 plugin.toml 的子目录后点击「重新扫描」。")
                    .size(13)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary),
            )
            .padding(20)
            .width(Length::Fill)
            .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0)),
        );
    } else {
        for plugin in plugins {
            let name = plugin.manifest.name.clone();
            let hooks = plugin.manifest.hooks.join(", ");
            let patterns = if plugin.manifest.url_patterns.is_empty() {
                "*".to_string()
            } else {
                plugin.manifest.url_patterns.join(", ")
            };
            let cmd = plugin.manifest.command.join(" ");

            let card = container(
                row![
                    toggler(plugin.manifest.enabled)
                        .on_toggle(move |v| Message::TogglePlugin(name.clone(), v)),
                    column![
                        row![
                            text(&plugin.manifest.name)
                                .size(14)
                                .font(FONT_MEDIUM)
                                .color(palette.text_primary),
                            text(format!("v{}", plugin.manifest.version))
                                .size(12)
                                .font(FONT_MONO)
                                .color(palette.text_muted),
                            text(format!("hooks: {hooks}"))
                                .size(12)
                                .font(FONT_MONO)
                                .color(palette.text_secondary),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                        text(&plugin.manifest.description)
                            .size(13)
                            .font(FONT_REGULAR)
                            .color(palette.text_secondary),
                        text(format!("匹配: {patterns}  |  命令: {cmd}"))
                            .size(12)
                            .font(FONT_MONO)
                            .color(palette.text_muted),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                ]
                .spacing(14)
                .align_y(Alignment::Center),
            )
            .padding([16, 18])
            .width(Length::Fill)
            .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0));

            list_col = list_col.push(card);
        }
    }

    let guide_snippet = r#"name = "my-resolver"
version = "0.1.0"
enabled = true
hooks = ["resolve", "post_download"]
url_patterns = ["*://example.com/*"]
command = ["python", "main.py"]
timeout_secs = 15"#;

    let guide_card = container(
        column![
            text("plugin.toml 协议示例")
                .size(14)
                .font(FONT_MEDIUM)
                .color(palette.text_primary),
            container(
                text(guide_snippet)
                    .size(12)
                    .font(FONT_MONO)
                    .color(palette.text_primary)
            )
            .padding(14)
            .width(Length::Fill)
            .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
            text("resolve 钩子通过 stdin 接收 JSON 请求，向 stdout 返回解析后的直链、Header、文件名或 extra_tasks 子任务列表。")
                .size(13)
                .font(FONT_REGULAR)
                .color(palette.text_secondary),
        ]
        .spacing(10),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    let body = column![header, browser_ext_card, dir_info, list_col, guide_card]
        .spacing(16)
        .padding([24, 28]);

    container(
        scrollable(body)
            .height(Length::Fill)
            .style(clean_scrollable_style(palette)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(panel_style(palette.bg_main, None, 0.0))
    .into()
}
