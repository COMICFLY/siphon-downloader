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
    let mut swatches = row![].spacing(7).align_y(Alignment::Center);
    for (_name, hex) in ACCENT_PRESETS {
        let Some((r, g, b)) = parse_hex_rgb(hex) else {
            continue;
        };
        let selected = current_hex.eq_ignore_ascii_case(hex);
        swatches = swatches.push(
            button(space())
                .width(Length::Fixed(22.0))
                .height(Length::Fixed(22.0))
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
                .padding([6, 8])
                .width(Length::Fixed(92.0))
                .style(input_style(palette)),
            input_target
        ),
        button(text("应用").size(12).font(FONT_MEDIUM))
            .padding([6, 10])
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
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let header = row![
        column![
            text("设置")
                .size(22)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            text("配置保存至 %APPDATA%\\Siphon\\config.toml")
                .size(14)
                .font(FONT_REGULAR)
                .color(palette.text_secondary),
        ]
        .spacing(4),
        space::horizontal(),
        button(text("保存设置").size(13).font(FONT_MEDIUM))
            .padding([8, 16])
            .style(primary_btn_style(palette))
            .on_press(Message::SaveSettings),
    ]
    .align_y(Alignment::Center);

    let choice_btn = |label: &'static str, active: bool, msg: Message| {
        button(
            text(label)
                .size(13)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR }),
        )
        .padding([6, 12])
        .style(subtle_btn_style(palette, active))
        .on_press(msg)
    };

    let appearance_card = settings_card(
        "外观与窗口",
        column![
            setting_row(
                "颜色主题",
                "中性石墨灰深浅双主题",
                row![
                    choice_btn("深色", cfg.theme_mode == ThemeMode::Dark, Message::SetThemeMode(ThemeMode::Dark)),
                    choice_btn("浅色", cfg.theme_mode == ThemeMode::Light, Message::SetThemeMode(ThemeMode::Light)),
                    choice_btn("跟随系统", cfg.theme_mode == ThemeMode::System, Message::SetThemeMode(ThemeMode::System)),
                ]
                .spacing(4),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "主界面主题色",
                "自定义全局进度条、速率曲线与高亮状态色（支持预设或任意 HEX 色值）",
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
                "仅控制独立下载详情小窗内的进度条、实时速率与分片数据色彩（默认翠绿 #22C55E）",
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
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "Windows 11 Mica 材质",
                "关闭后切换为纯净实色表面",
                toggler(cfg.use_mica).on_toggle(Message::ToggleMica),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "任务列表密度",
                "调整任务列表行高与信息排布",
                row![
                    choice_btn("舒适", cfg.list_density == ListDensity::Comfortable, Message::SetDensity(ListDensity::Comfortable)),
                    choice_btn("紧凑", cfg.list_density == ListDensity::Compact, Message::SetDensity(ListDensity::Compact)),
                ]
                .spacing(4),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "界面语言",
                "简体中文默认，可切换英文",
                row![
                    choice_btn("简体中文", cfg.language == Language::ZhCn, Message::SetLanguage(Language::ZhCn)),
                    choice_btn("English", cfg.language == Language::EnUs, Message::SetLanguage(Language::EnUs)),
                ]
                .spacing(4),
                palette,
            ),
        ]
        .spacing(14),
        palette,
    );

    let split_mib = (cfg.min_split_size_bytes / (1024 * 1024)).max(1);
    let engine_card = settings_card(
        "下载引擎",
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
                            .padding([8, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::SettingsSaveDir
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
                    .on_press(Message::BrowseDefaultSaveDir),
                ]
                .spacing(8),
            ]
            .spacing(6),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "默认单任务并发线程",
                "动态二分借片（1–32，推荐 8）",
                row![
                    container(
                        slider(
                            1.0_f32..=32.0_f32,
                            cfg.default_connections as f32,
                            Message::SetDefaultConnections
                        )
                        .step(1.0_f32)
                    )
                    .width(Length::Fixed(160.0)),
                    text(format!("{} 线程", cfg.default_connections))
                        .size(13)
                        .font(FONT_MONO)
                        .wrapping(text::Wrapping::None)
                        .width(Length::Fixed(64.0))
                        .align_x(iced::alignment::Horizontal::Right)
                        .color(palette.text_primary),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "最大同时活跃任务数",
                "超出上限的新任务进入排队队列",
                row![
                    container(
                        slider(
                            1.0_f32..=12.0_f32,
                            cfg.max_concurrent_tasks as f32,
                            Message::SetMaxConcurrentTasks
                        )
                        .step(1.0_f32)
                    )
                    .width(Length::Fixed(160.0)),
                    text(format!("{} 个", cfg.max_concurrent_tasks))
                        .size(13)
                        .font(FONT_MONO)
                        .wrapping(text::Wrapping::None)
                        .width(Length::Fixed(64.0))
                        .align_x(iced::alignment::Horizontal::Right)
                        .color(palette.text_primary),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "动态借片最小阈值",
                "剩余未下字节超过该值时由空闲线程接管后半段",
                row![
                    choice_btn("1 MiB", split_mib == 1, Message::SetMinSplitMiB(1)),
                    choice_btn("2 MiB", split_mib == 2, Message::SetMinSplitMiB(2)),
                    choice_btn("4 MiB", split_mib == 4, Message::SetMinSplitMiB(4)),
                ]
                .spacing(4),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "全局限速 (KB/s)",
                "0 表示不限速，点击应用立即热生效",
                row![
                    wrap_input(
                        text_input("0", global_speed_input)
                            .on_input(Message::SetGlobalSpeedLimitInput)
                            .on_submit(Message::ApplyGlobalSpeedLimit)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fixed(120.0))
                            .style(input_style(palette)),
                        InputTarget::SettingsGlobalSpeed
                    ),
                    button(text("应用").size(13).font(FONT_MEDIUM))
                        .padding([7, 14])
                        .style(outline_btn_style(palette))
                        .on_press(Message::ApplyGlobalSpeedLimit),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                palette,
            ),
        ]
        .spacing(14),
        palette,
    );

    let browser_card = settings_card(
        "浏览器与剪贴板下载捕获（IDM 风格）",
        column![
            setting_row(
                "接管浏览器下载请求",
                "监听本地 127.0.0.1:16800 接收 Chrome / Edge 扩展拦截的下载（自动携带 Cookie 与 Referer）",
                toggler(cfg.browser_integration).on_toggle(Message::ToggleBrowserIntegration),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "浏览器捕获时弹出独立悬浮小窗",
                "类似 IDM：主窗口最小化在托盘时仅弹出独立置顶捕获小窗；关闭则按规则静默直下",
                toggler(cfg.browser_capture_popup).on_toggle(Message::ToggleBrowserCapturePopup),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "剪贴板复制链接时直接弹出独立捕获小窗",
                "开启后复制下载直链立即弹出独立捕获小窗；关闭则仅在主界面内显示温和提示条",
                toggler(cfg.clipboard_capture_popup)
                    .on_toggle(Message::ToggleClipboardCapturePopup),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "Chrome / Edge 浏览器捕获插件",
                if browser_ext_connected {
                    "已检测到浏览器扩展实时连接 — 可随时重新打开安装向导为其他浏览器安装"
                } else {
                    "打开引导式安装向导：自动识别默认浏览器、部署高清图标扩展并实时检测连接状态"
                },
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
                            text("扩展文件夹").size(13).font(FONT_REGULAR),
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
                            text("安装向导").size(13).font(FONT_MEDIUM),
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
                palette,
            ),
        ]
        .spacing(14),
        palette,
    );

    let delete_card = settings_card(
        "删除与确认",
        column![
            setting_row(
                "删除任务时弹窗确认",
                "点击删除按钮时询问是否同时删除本地磁盘文件",
                toggler(cfg.confirm_on_delete).on_toggle(Message::ToggleConfirmOnDelete),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "删除任务默认选项",
                "确认弹窗的默认勾选项；关闭弹窗确认时直接按此选项执行",
                row![
                    choice_btn(
                        "仅移除记录",
                        !cfg.delete_files_by_default,
                        Message::SetDeleteFilesByDefault(false)
                    ),
                    choice_btn(
                        "同时删除文件",
                        cfg.delete_files_by_default,
                        Message::SetDeleteFilesByDefault(true)
                    ),
                ]
                .spacing(4),
                palette,
            ),
        ]
        .spacing(14),
        palette,
    );

    let system_card = settings_card(
        "网络与系统集成",
        column![
            column![
                text("代理地址（留空自动读取 Windows 系统代理）")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                wrap_input(
                    text_input("http://127.0.0.1:7890 或 socks5://127.0.0.1:1080", proxy_input)
                        .on_input(Message::SetProxyUrlInput)
                        .font(FONT_MONO)
                        .size(13)
                        .padding([8, 10])
                        .style(input_style(palette)),
                    InputTarget::SettingsProxy
                ),
            ]
            .spacing(6),
            column![
                text("User-Agent")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                wrap_input(
                    text_input("", ua_input)
                        .on_input(Message::SetUserAgentInput)
                        .font(FONT_MONO)
                        .size(13)
                        .padding([8, 10])
                        .style(input_style(palette)),
                    InputTarget::SettingsUserAgent
                ),
            ]
            .spacing(6),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "剪贴板链接监听",
                "Win32 事件驱动监听，复制直链后温和提示",
                toggler(cfg.clipboard_monitor).on_toggle(Message::ToggleClipboardMonitor),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "关闭窗口时最小化到托盘",
                "保持后台持续传输，右键托盘可退出",
                toggler(cfg.minimize_to_tray).on_toggle(Message::ToggleMinimizeToTray),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "开机静默启动",
                "随 Windows 登录启动并常驻托盘",
                toggler(cfg.autostart).on_toggle(Message::ToggleAutostart),
                palette,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            setting_row(
                "软件数据目录（config.toml / siphon.db / plugins）",
                "可自由迁移到任意磁盘目录，也支持通过安装包或 --data-dir 指定独立目录",
                row![
                    button(
                        row![
                            icon(IconKind::Folder, 14.0, palette.text_secondary),
                            text("打开目录").size(13).font(FONT_REGULAR),
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
                            text("更改目录").size(13).font(FONT_REGULAR),
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
                palette,
            ),
        ]
        .spacing(14),
        palette,
    );

    let body = column![
        header,
        appearance_card,
        engine_card,
        browser_card,
        delete_card,
        system_card
    ]
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

fn settings_card<'a>(
    title: &'static str,
    content: impl Into<Element<'a, Message>>,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(14)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            content.into(),
        ]
        .spacing(14),
    )
    .padding(20)
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
