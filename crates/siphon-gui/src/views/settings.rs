use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message};
use crate::theme::{
    ACCENT_PRESETS, FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette,
    clean_scrollable_style, hairline_rule_style, input_style, outline_btn_style, panel_style,
    parse_hex_rgb, primary_btn_style, subtle_btn_style, swatch_btn_style,
};
use iced::widget::{
    button, column, container, mouse_area, row, rule, scrollable, slider, space, text, text_input,
    toggler,
};
use iced::{Alignment, Color, Element, Length};
use siphon_core::{AppConfig, Language, ListDensity, ThemeMode};
use std::path::Path;

fn wrap_input<'a>(
    input: impl Into<Element<'a, Message>>,
    target: InputTarget,
) -> Element<'a, Message> {
    mouse_area(input)
        .on_release(Message::FocusInputTarget(target))
        .on_right_press(Message::OpenInputContextMenu(target))
        .into()
}

fn color_picker_row<'a>(
    current_hex: &str,
    hex_input: &'a str,
    input_target: InputTarget,
    on_preset: impl Fn(String) -> Message,
    on_input: impl Fn(String) -> Message + 'static,
    on_apply: Message,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let mut swatches = row![].spacing(6).align_y(Alignment::Center);
    for (_name, hex) in ACCENT_PRESETS {
        let Some((r, g, b)) = parse_hex_rgb(hex) else {
            continue;
        };
        let selected = current_hex.eq_ignore_ascii_case(hex);
        swatches = swatches.push(
            button(space())
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(20.0))
                .padding(0)
                .style(swatch_btn_style(Color::from_rgb8(r, g, b), selected, palette))
                .on_press(on_preset((*hex).to_string())),
        );
    }

    let preview_col = parse_hex_rgb(hex_input)
        .or_else(|| parse_hex_rgb(current_hex))
        .map(|(r, g, b)| Color::from_rgb8(r, g, b))
        .unwrap_or(palette.accent);

    row![
        swatches,
        container(space())
            .width(Length::Fixed(14.0))
            .height(Length::Fixed(14.0))
            .style(panel_style(preview_col, Some(palette.hairline_strong), 7.0)),
        wrap_input(
            text_input("#38BDF8", hex_input)
                .on_input(on_input)
                .on_submit(on_apply.clone())
                .font(FONT_MONO)
                .size(12)
                .padding([5, 8])
                .width(Length::Fixed(86.0))
                .style(input_style(palette)),
            input_target
        ),
        button(
            text("应用")
                .size(12)
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::None),
        )
        .padding([5, 10])
        .style(outline_btn_style(palette))
        .on_press(on_apply),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    cfg: &'a AppConfig,
    save_dir_input: &'a str,
    global_speed_input: &'a str,
    proxy_input: &'a str,
    ua_input: &'a str,
    accent_hex_input: &'a str,
    popup_accent_hex_input: &'a str,
    browser_ext_connected: bool,
    data_dir: &'a Path,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let config_path_display = data_dir.join("config.toml").to_string_lossy().to_string();
    let header = row![
        column![
            text("设置")
                .size(22)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            text(format!("配置文件：{config_path_display}"))
                .size(13)
                .font(FONT_MONO)
                .color(palette.text_muted),
        ]
        .spacing(4),
        space::horizontal(),
        button(
            text("保存设置")
                .size(13)
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::None),
        )
        .padding([8, 16])
        .style(primary_btn_style(palette))
        .on_press(Message::SaveSettings),
    ]
    .align_y(Alignment::Center);

    let choice_btn = |label: &'static str, active: bool, msg: Message| {
        button(
            text(label)
                .size(12)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                .wrapping(text::Wrapping::None),
        )
        .padding([5, 11])
        .style(subtle_btn_style(palette, active))
        .on_press(msg)
    };

    let segmented = |buttons: iced::widget::Row<'a, Message>| {
        container(buttons.spacing(2).align_y(Alignment::Center))
            .padding(2)
            .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0))
    };

    // Card 1: General & Appearance (Theme, Mica, Layout Density, Language, Dual Accent Colors)
    let appearance_card = settings_card(
        IconKind::Sun,
        "常规与外观",
        None,
        column![
            setting_row(
                "颜色主题与材质",
                "中性石墨灰深浅双主题，支持 Windows 11 Mica 半透明材质",
                row![
                    segmented(row![
                        choice_btn(
                            "深色",
                            cfg.theme_mode == ThemeMode::Dark,
                            Message::SetThemeMode(ThemeMode::Dark),
                        ),
                        choice_btn(
                            "浅色",
                            cfg.theme_mode == ThemeMode::Light,
                            Message::SetThemeMode(ThemeMode::Light),
                        ),
                        choice_btn(
                            "跟随系统",
                            cfg.theme_mode == ThemeMode::System,
                            Message::SetThemeMode(ThemeMode::System),
                        ),
                    ]),
                    row![
                        text("Mica")
                            .size(12)
                            .font(FONT_MEDIUM)
                            .color(palette.text_secondary),
                        toggler(cfg.use_mica).on_toggle(Message::ToggleMica).size(17),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "列表密度与界面语言",
                "自定义任务列表行高与界面显示语言",
                row![
                    segmented(row![
                        choice_btn(
                            "舒适",
                            cfg.list_density == ListDensity::Comfortable,
                            Message::SetDensity(ListDensity::Comfortable),
                        ),
                        choice_btn(
                            "紧凑",
                            cfg.list_density == ListDensity::Compact,
                            Message::SetDensity(ListDensity::Compact),
                        ),
                    ]),
                    segmented(row![
                        choice_btn(
                            "简体中文",
                            cfg.language == Language::ZhCn,
                            Message::SetLanguage(Language::ZhCn),
                        ),
                        choice_btn(
                            "English",
                            cfg.language == Language::EnUs,
                            Message::SetLanguage(Language::EnUs),
                        ),
                    ]),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "主界面主题色",
                "全局进度条、速率曲线与选中高亮色（支持预设或 HEX）",
                color_picker_row(
                    &cfg.accent_color,
                    accent_hex_input,
                    InputTarget::SettingsAccentHex,
                    Message::SetAccentPreset,
                    Message::SetAccentHexInput,
                    Message::ApplyAccentHex,
                    palette,
                ),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "独立下载弹窗数据色",
                "独立下载进度小窗内的进度条与实时速率色彩（默认翠绿）",
                color_picker_row(
                    &cfg.download_popup_accent,
                    popup_accent_hex_input,
                    InputTarget::SettingsPopupAccentHex,
                    Message::SetDownloadPopupAccentPreset,
                    Message::SetDownloadPopupAccentHexInput,
                    Message::ApplyDownloadPopupAccentHex,
                    palette,
                ),
                palette,
            ),
        ]
        .spacing(12),
        palette,
    );

    // Card 2: Downloads, Concurrency, Speed Cap & Delete Behavior
    let split_mib = (cfg.min_split_size_bytes / (1024 * 1024)).max(1);
    let downloads_card = settings_card(
        IconKind::Downloading,
        "下载与任务行为",
        None,
        column![
            column![
                text("默认保存目录")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .color(palette.text_primary),
                row![
                    wrap_input(
                        text_input("", save_dir_input)
                            .on_input(Message::SetDefaultSaveDirInput)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::SettingsSaveDir
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
                    .padding([7, 14])
                    .style(outline_btn_style(palette))
                    .on_press(Message::BrowseDefaultSaveDir),
                ]
                .spacing(8),
            ]
            .spacing(6),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            row![
                container(
                    column![
                        row![
                            text("默认单任务线程数")
                                .size(13)
                                .font(FONT_MEDIUM)
                                .color(palette.text_primary),
                            space::horizontal(),
                            text(format!("{} 线程", cfg.default_connections))
                                .size(12)
                                .font(FONT_MONO)
                                .color(palette.accent),
                        ]
                        .align_y(Alignment::Center),
                        slider(
                            1.0_f32..=32.0_f32,
                            cfg.default_connections as f32,
                            Message::SetDefaultConnections
                        )
                        .step(1.0_f32),
                        text("动态二分借片（1–32，推荐 8–16）")
                            .size(11)
                            .font(FONT_REGULAR)
                            .color(palette.text_muted),
                    ]
                    .spacing(6),
                )
                .padding([10, 12])
                .width(Length::Fill)
                .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
                container(
                    column![
                        row![
                            text("最大同时活跃任务")
                                .size(13)
                                .font(FONT_MEDIUM)
                                .color(palette.text_primary),
                            space::horizontal(),
                            text(format!("{} 个", cfg.max_concurrent_tasks))
                                .size(12)
                                .font(FONT_MONO)
                                .color(palette.accent),
                        ]
                        .align_y(Alignment::Center),
                        slider(
                            1.0_f32..=12.0_f32,
                            cfg.max_concurrent_tasks as f32,
                            Message::SetMaxConcurrentTasks
                        )
                        .step(1.0_f32),
                        text("超出上限的新任务自动进入排队")
                            .size(11)
                            .font(FONT_REGULAR)
                            .color(palette.text_muted),
                    ]
                    .spacing(6),
                )
                .padding([10, 12])
                .width(Length::Fill)
                .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
            ]
            .spacing(10),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "动态借片最小阈值",
                "剩余未下字节超过该阈值时由空闲线程接管后半段",
                segmented(row![
                    choice_btn("1 MiB", split_mib == 1, Message::SetMinSplitMiB(1)),
                    choice_btn("2 MiB", split_mib == 2, Message::SetMinSplitMiB(2)),
                    choice_btn("4 MiB", split_mib == 4, Message::SetMinSplitMiB(4)),
                ]),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "全局下载限速 (KB/s)",
                "0 表示不限速，点击应用立即热生效",
                row![
                    wrap_input(
                        text_input("0", global_speed_input)
                            .on_input(Message::SetGlobalSpeedLimitInput)
                            .on_submit(Message::ApplyGlobalSpeedLimit)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([6, 10])
                            .width(Length::Fixed(116.0))
                            .style(input_style(palette)),
                        InputTarget::SettingsGlobalSpeed
                    ),
                    button(
                        text("应用")
                            .size(12)
                            .font(FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([6, 14])
                    .style(outline_btn_style(palette))
                    .on_press(Message::ApplyGlobalSpeedLimit),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "删除任务确认与默认动作",
                "删除记录时是否弹窗确认，以及默认是否同时清理本地文件",
                row![
                    row![
                        text("删除确认")
                            .size(12)
                            .font(FONT_REGULAR)
                            .color(palette.text_secondary),
                        toggler(cfg.confirm_on_delete)
                            .on_toggle(Message::ToggleConfirmOnDelete)
                            .size(17),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                    segmented(row![
                        choice_btn(
                            "仅移除记录",
                            !cfg.delete_files_by_default,
                            Message::SetDeleteFilesByDefault(false),
                        ),
                        choice_btn(
                            "同时删文件",
                            cfg.delete_files_by_default,
                            Message::SetDeleteFilesByDefault(true),
                        ),
                    ]),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                palette,
            ),
        ]
        .spacing(12),
        palette,
    );

    // Card 3: Browser & Clipboard Capture (all capture & extension settings unified cleanly)
    let ext_badge = if browser_ext_connected {
        Some(("扩展已连接", palette.ok, palette.ok_soft))
    } else {
        Some(("扩展未连接", palette.text_muted, palette.bg_input))
    };

    let capture_card = settings_card(
        IconKind::Plugins,
        "浏览器与剪贴板捕获",
        ext_badge,
        column![
            setting_row(
                "接管浏览器下载请求",
                "监听本地 127.0.0.1:16800 接收 Chrome / Edge 扩展拦截的下载（自动携带 Cookie 与 Referer）",
                toggler(cfg.browser_integration)
                    .on_toggle(Message::ToggleBrowserIntegration)
                    .size(18),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "浏览器捕获时弹出独立悬浮小窗",
                "主窗口最小化常驻托盘时弹出置顶捕获小窗；关闭则按规则静默直下",
                toggler(cfg.browser_capture_popup)
                    .on_toggle(Message::ToggleBrowserCapturePopup)
                    .size(18),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "剪贴板链接监听",
                "Win32 事件驱动监听，复制可下载直链时自动提醒",
                toggler(cfg.clipboard_monitor)
                    .on_toggle(Message::ToggleClipboardMonitor)
                    .size(18),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "剪贴板捕获直接弹出独立小窗",
                "开启后复制链接立即弹出捕获小窗；关闭则仅在主界面顶部显示温和提示条",
                toggler(cfg.clipboard_capture_popup)
                    .on_toggle(Message::ToggleClipboardCapturePopup)
                    .size(18),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "Chrome / Edge 浏览器扩展部署",
                if browser_ext_connected {
                    "已检测到扩展实时连接，可随时导出扩展包或为其他浏览器重新安装"
                } else {
                    "启动引导式安装向导，或直接导出 .zip / 打开扩展目录手动加载"
                },
                row![
                    button(
                        row![
                            icon(IconKind::Archive, 13.0, palette.text_secondary),
                            text("导出 .zip")
                                .size(12)
                                .font(FONT_REGULAR)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 11])
                    .style(outline_btn_style(palette))
                    .on_press(Message::ExportBrowserExtensionZip),
                    button(
                        row![
                            icon(IconKind::Folder, 13.0, palette.text_secondary),
                            text("扩展目录")
                                .size(12)
                                .font(FONT_REGULAR)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 11])
                    .style(outline_btn_style(palette))
                    .on_press(Message::OpenBrowserExtensionFolder),
                    button(
                        row![
                            icon(IconKind::Plugins, 13.0, palette.cta_text),
                            text("安装向导")
                                .size(12)
                                .font(FONT_MEDIUM)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 13])
                    .style(primary_btn_style(palette))
                    .on_press(Message::InstallBrowserExtension),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                palette,
            ),
        ]
        .spacing(12),
        palette,
    );

    // Card 4: Network, System Tray, Autostart & Data Directory
    let data_dir_str = data_dir.to_string_lossy().to_string();
    let system_card = settings_card(
        IconKind::Settings,
        "网络与系统",
        None,
        column![
            row![
                column![
                    text("代理地址（留空自动读取系统代理）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("http://127.0.0.1:7890 或 socks5://127.0.0.1:1080", proxy_input)
                            .on_input(Message::SetProxyUrlInput)
                            .font(FONT_MONO)
                            .size(12)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::SettingsProxy
                    ),
                ]
                .spacing(5)
                .width(Length::Fill),
                column![
                    text("User-Agent")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("", ua_input)
                            .on_input(Message::SetUserAgentInput)
                            .font(FONT_MONO)
                            .size(12)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::SettingsUserAgent
                    ),
                ]
                .spacing(5)
                .width(Length::Fill),
            ]
            .spacing(12),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "关闭窗口时最小化到托盘",
                "保持后台持续下载与做种，右键系统托盘图标可完全退出",
                toggler(cfg.minimize_to_tray)
                    .on_toggle(Message::ToggleMinimizeToTray)
                    .size(18),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "开机静默启动",
                "随 Windows 登录自动启动并静默常驻托盘",
                toggler(cfg.autostart)
                    .on_toggle(Message::ToggleAutostart)
                    .size(18),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            row![
                column![
                    text("软件数据目录（配置 / 数据库 / 插件）")
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text(data_dir_str)
                        .size(12)
                        .font(FONT_MONO)
                        .color(palette.text_muted),
                ]
                .spacing(3)
                .width(Length::Fill),
                row![
                    button(
                        row![
                            icon(IconKind::Folder, 13.0, palette.text_secondary),
                            text("打开目录")
                                .size(12)
                                .font(FONT_REGULAR)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 11])
                    .style(outline_btn_style(palette))
                    .on_press(Message::OpenDataDir),
                    button(
                        row![
                            icon(IconKind::Edit, 12.0, palette.text_secondary),
                            text("更改目录")
                                .size(12)
                                .font(FONT_REGULAR)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 11])
                    .style(outline_btn_style(palette))
                    .on_press(Message::ChangeDataDir),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(12),
        palette,
    );

    let body = column![
        header,
        appearance_card,
        downloads_card,
        capture_card,
        system_card,
    ]
    .spacing(14)
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

fn settings_card<'a>(
    card_icon: IconKind,
    title: &'static str,
    badge: Option<(&'static str, Color, Color)>,
    content: impl Into<Element<'a, Message>>,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let mut title_row = row![
        icon(card_icon, 15.0, palette.accent),
        text(title)
            .size(14)
            .font(FONT_SEMIBOLD)
            .color(palette.text_primary),
        space::horizontal(),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if let Some((label, fg, bg)) = badge {
        title_row = title_row.push(
            container(
                text(label)
                    .size(11)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None)
                    .color(fg),
            )
            .padding([2, 8])
            .style(panel_style(bg, None, 5.0)),
        );
    }

    container(column![title_row, content.into()].spacing(14))
        .padding(18)
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0))
        .into()
}

fn setting_row<'a>(
    title: &'static str,
    subtitle: &'static str,
    control: impl Into<Element<'a, Message>>,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    row![
        column![
            text(title)
                .size(13)
                .font(FONT_MEDIUM)
                .color(palette.text_primary),
            text(subtitle)
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
        ]
        .spacing(3)
        .width(Length::Fill),
        control.into(),
    ]
    .spacing(16)
    .align_y(Alignment::Center)
    .into()
}
