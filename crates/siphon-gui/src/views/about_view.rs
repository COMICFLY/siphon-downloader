use crate::icons::{IconKind, icon};
use crate::state::{Message, UpdateCheckStatus};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, clean_scrollable_style,
    hairline_rule_style, outline_btn_style, panel_style, primary_btn_style, progress_style,
};
use iced::widget::{
    button, column, container, image, progress_bar, row, rule, scrollable, space, text,
};
use iced::{Alignment, Element, Length};
use siphon_core::{DownloadTask, TaskStatus, format_bytes, format_speed};
use siphon_engine_http::{GITHUB_RELEASES_URL, GITHUB_REPO_URL, GITHUB_USER_URL};
use std::path::Path;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn view<'a>(
    app_icon: &'a image::Handle,
    author_avatar: &'a image::Handle,
    update_status: &'a UpdateCheckStatus,
    auto_update_task: Option<&'a DownloadTask>,
    data_dir: &Path,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let header = column![
        text("关于软件")
            .size(22)
            .font(FONT_SEMIBOLD)
            .color(palette.text_primary),
        text("版本信息、在线检查更新、开发者主页与可控数据目录")
            .size(14)
            .font(FONT_REGULAR)
            .color(palette.text_secondary),
    ]
    .spacing(4);

    let checking = matches!(update_status, UpdateCheckStatus::Checking);
    let check_btn = button(
        row![
            icon(IconKind::Refresh, 14.0, palette.cta_text),
            text(if checking { "正在检查更新..." } else { "检查更新" })
                .size(13)
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([8, 16])
    .style(primary_btn_style(palette))
    .on_press_maybe((!checking).then_some(Message::CheckForUpdates));

    let hero_top = row![
        image(app_icon.clone())
            .width(Length::Fixed(56.0))
            .height(Length::Fixed(56.0))
            .filter_method(image::FilterMethod::Linear),
        column![
            row![
                text("Siphon Downloader")
                    .size(18)
                    .font(FONT_SEMIBOLD)
                    .color(palette.text_primary),
                container(
                    text(format!("v{APP_VERSION}"))
                        .size(12)
                        .font(FONT_MONO)
                        .color(palette.accent),
                )
                .padding([2, 8])
                .style(panel_style(palette.accent_soft, None, 5.0)),
                container(
                    text("MIT")
                        .size(11)
                        .font(FONT_MONO)
                        .color(palette.text_secondary),
                )
                .padding([2, 7])
                .style(panel_style(palette.bg_input, Some(palette.hairline), 5.0)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text("纯 Rust 编写的 Windows 11 高性能多线程下载器 · HTTP 动态二分借片 · HLS 流媒体合并 · BitTorrent / 磁力 · 计划调度与批量队列")
                .size(13)
                .font(FONT_REGULAR)
                .color(palette.text_secondary),
        ]
        .spacing(6)
        .width(Length::Fill),
        check_btn,
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    let mut hero_col = column![hero_top].spacing(14);

    match update_status {
        UpdateCheckStatus::Idle => {}
        UpdateCheckStatus::Checking => {
            hero_col = hero_col.push(
                container(
                    text("正在连接 GitHub Releases (COMICFLY/siphon-downloader) 查询最新版本...")
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                )
                .padding([10, 14])
                .width(Length::Fill)
                .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
            );
        }
        UpdateCheckStatus::UpToDate(ver) => {
            hero_col = hero_col.push(
                container(
                    row![
                        icon(IconKind::Completed, 15.0, palette.ok),
                        text(format!("当前已是最新版本（v{ver}）"))
                            .size(13)
                            .font(FONT_MEDIUM)
                            .color(palette.ok),
                        space::horizontal(),
                        button(
                            row![
                                text("查看发行版历史").size(12).font(FONT_REGULAR),
                                icon(IconKind::ExternalLink, 12.0, palette.text_secondary),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        )
                        .padding([5, 10])
                        .style(outline_btn_style(palette))
                        .on_press(Message::OpenExternalUrl(GITHUB_RELEASES_URL.to_string())),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
                .width(Length::Fill)
                .style(panel_style(palette.ok_soft, Some(palette.hairline), 8.0)),
            );
        }
        UpdateCheckStatus::UpdateAvailable(info) => {
            let notes_preview: String = info
                .release_notes
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(4)
                .collect::<Vec<_>>()
                .join("\n");

            let mut action_row = row![
                button(
                    row![
                        text("打开发行版页面").size(12).font(FONT_REGULAR),
                        icon(IconKind::ExternalLink, 12.0, palette.text_secondary),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([6, 12])
                .style(outline_btn_style(palette))
                .on_press(Message::OpenExternalUrl(info.release_url.clone())),
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            if let Some(installer_url) = &info.installer_url {
                let updating = auto_update_task.is_some();
                action_row = action_row
                    .push(
                        button(text("仅下载安装包").size(12).font(FONT_REGULAR))
                            .padding([6, 12])
                            .style(outline_btn_style(palette))
                            .on_press_maybe(
                                (!updating)
                                    .then_some(Message::DownloadUpdateInstaller(installer_url.clone())),
                            ),
                    )
                    .push(
                        button(
                            row![
                                icon(IconKind::Bolt, 13.0, palette.cta_text),
                                text(if updating {
                                    "正在下载更新..."
                                } else {
                                    "一键下载并自动安装"
                                })
                                .size(12)
                                .font(FONT_MEDIUM),
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center),
                        )
                        .padding([6, 14])
                        .style(primary_btn_style(palette))
                        .on_press_maybe(
                            (!updating).then_some(Message::StartAutoUpdate(installer_url.clone())),
                        ),
                    );
            }

            let mut update_box = column![
                row![
                    icon(IconKind::Bolt, 15.0, palette.accent),
                    text(format!(
                        "发现新版本 v{}（{}）",
                        info.latest_version, info.release_name
                    ))
                    .size(13)
                    .font(FONT_SEMIBOLD)
                    .color(palette.accent),
                    space::horizontal(),
                    action_row,
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(8);

            if !notes_preview.is_empty() {
                update_box = update_box.push(
                    text(notes_preview)
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                );
            }

            if let Some(task) = auto_update_task {
                let ratio = task.progress_ratio();
                let pct = (ratio * 100.0).clamp(0.0, 100.0);
                let size_txt = match task.total_size {
                    Some(total) => format!(
                        "{} / {}",
                        format_bytes(task.downloaded_bytes),
                        format_bytes(total)
                    ),
                    None => format_bytes(task.downloaded_bytes),
                };
                let status_hint = if task.status == TaskStatus::Completed {
                    "更新包下载完成，正在启动静默安装程序并重启 Siphon...".to_string()
                } else {
                    format!(
                        "正在自动下载更新包 ({pct:.1}%) · {} · 下载完成后将自动静默升级并重启",
                        format_speed(task.speed_bps)
                    )
                };
                update_box = update_box.push(
                    container(
                        column![
                            row![
                                text(status_hint)
                                    .size(12)
                                    .font(FONT_MEDIUM)
                                    .color(palette.text_primary),
                                space::horizontal(),
                                text(size_txt)
                                    .size(12)
                                    .font(FONT_MONO)
                                    .color(palette.text_secondary),
                            ]
                            .align_y(Alignment::Center),
                            progress_bar(0.0..=1.0, ratio)
                                .length(Length::Fill)
                                .girth(Length::Fixed(6.0))
                                .style(progress_style(palette, palette.accent)),
                        ]
                        .spacing(8),
                    )
                    .padding([10, 12])
                    .width(Length::Fill)
                    .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
                );
            }

            hero_col = hero_col.push(
                container(update_box)
                    .padding([12, 14])
                    .width(Length::Fill)
                    .style(panel_style(
                        palette.accent_soft,
                        Some(palette.hairline_strong),
                        8.0,
                    )),
            );
        }
        UpdateCheckStatus::Failed(err) => {
            hero_col = hero_col.push(
                container(
                    row![
                        text(format!("检查更新失败：{err}"))
                            .size(12)
                            .font(FONT_REGULAR)
                            .color(palette.err)
                            .width(Length::Fill),
                        button(
                            row![
                                text("手动访问 Releases").size(12).font(FONT_REGULAR),
                                icon(IconKind::ExternalLink, 12.0, palette.text_secondary),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        )
                        .padding([5, 10])
                        .style(outline_btn_style(palette))
                        .on_press(Message::OpenExternalUrl(GITHUB_RELEASES_URL.to_string())),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
                .width(Length::Fill)
                .style(panel_style(palette.err_soft, None, 8.0)),
            );
        }
    }

    let hero_card = container(hero_col)
        .padding(20)
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Author card with GitHub avatar, name, and profile link
    let author_card = container(
        column![
            text("开发者与项目主页")
                .size(14)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            row![
                container(
                    image(author_avatar.clone())
                        .width(Length::Fixed(64.0))
                        .height(Length::Fixed(64.0))
                        .filter_method(image::FilterMethod::Linear),
                )
                .padding(2)
                .style(panel_style(
                    palette.bg_input,
                    Some(palette.hairline_strong),
                    34.0,
                )),
                column![
                    row![
                        text("COMICFLY")
                            .size(16)
                            .font(FONT_SEMIBOLD)
                            .color(palette.text_primary),
                        container(
                            text("Author")
                                .size(11)
                                .font(FONT_MONO)
                                .color(palette.accent),
                        )
                        .padding([2, 7])
                        .style(panel_style(palette.accent_soft, None, 5.0)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    text("GitHub 主页：https://github.com/COMICFLY")
                        .size(12)
                        .font(FONT_MONO)
                        .color(palette.text_secondary),
                    text("项目仓库：https://github.com/COMICFLY/siphon-downloader")
                        .size(12)
                        .font(FONT_MONO)
                        .color(palette.text_muted),
                ]
                .spacing(5)
                .width(Length::Fill),
                column![
                    button(
                        row![
                            text("访问 GitHub 主页")
                                .size(13)
                                .font(FONT_MEDIUM)
                                .wrapping(text::Wrapping::None),
                            icon(IconKind::ExternalLink, 13.0, palette.cta_text),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding([7, 14])
                    .style(primary_btn_style(palette))
                    .on_press(Message::OpenExternalUrl(GITHUB_USER_URL.to_string())),
                    button(
                        row![
                            text("打开项目仓库")
                                .size(13)
                                .font(FONT_REGULAR)
                                .wrapping(text::Wrapping::None),
                            icon(IconKind::ExternalLink, 13.0, palette.text_secondary),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding([7, 14])
                    .style(outline_btn_style(palette))
                    .on_press(Message::OpenExternalUrl(GITHUB_REPO_URL.to_string())),
                ]
                .spacing(8)
                .align_x(Alignment::End),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        ]
        .spacing(14),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Data directory & browser extension quick actions card
    let runtime_card = container(
        column![
            text("数据存储与浏览器扩展分发")
                .size(14)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            row![
                column![
                    text("当前数据目录（config.toml / siphon.db / plugins）")
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text(data_dir.to_string_lossy().to_string())
                        .size(12)
                        .font(FONT_MONO)
                        .color(palette.text_secondary),
                ]
                .spacing(4)
                .width(Length::Fill),
                button(
                    row![
                        icon(IconKind::Folder, 14.0, palette.text_secondary),
                        text("打开数据目录").size(13).font(FONT_REGULAR),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 12])
                .style(outline_btn_style(palette))
                .on_press(Message::OpenDataDir),
                button(
                    row![
                        icon(IconKind::Edit, 13.0, palette.text_secondary),
                        text("更改数据目录").size(13).font(FONT_REGULAR),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 12])
                .style(outline_btn_style(palette))
                .on_press(Message::ChangeDataDir),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            row![
                column![
                    text("浏览器扩展独立分发包")
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text("支持以独立文件夹加载，或导出标准 .zip 扩展压缩包分享与手动安装")
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted),
                ]
                .spacing(4)
                .width(Length::Fill),
                button(
                    row![
                        icon(IconKind::Archive, 14.0, palette.text_secondary),
                        text("导出插件压缩包 (.zip)").size(13).font(FONT_REGULAR),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 12])
                .style(outline_btn_style(palette))
                .on_press(Message::ExportBrowserExtensionZip),
                button(
                    row![
                        icon(IconKind::Plugins, 14.0, palette.text_secondary),
                        text("打开安装向导").size(13).font(FONT_REGULAR),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 12])
                .style(outline_btn_style(palette))
                .on_press(Message::InstallBrowserExtension),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(14),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    let body = column![header, hero_card, author_card, runtime_card]
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
