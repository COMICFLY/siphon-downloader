use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, clean_scrollable_style,
    hairline_rule_style, input_style, outline_btn_style, panel_style, primary_btn_style,
    subtle_btn_style,
};
use iced::widget::{
    button, column, container, mouse_area, row, rule, scrollable, space, text, text_input, toggler,
};
use iced::{Alignment, Element, Length};
use siphon_core::{QueueCompleteAction, ScheduleConfig, is_time_in_window, parse_hhmm};

fn wrap_input<'a>(
    input: impl Into<Element<'a, Message>>,
    target: InputTarget,
) -> Element<'a, Message> {
    mouse_area(input)
        .on_release(Message::FocusInputTarget(target))
        .on_right_press(Message::OpenInputContextMenu(target))
        .into()
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    sched: &'a ScheduleConfig,
    speed_kbps_input: &'a str,
    speed_window_active: bool,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let now_str = chrono::Local::now().format("%H:%M").to_string();

    let header = row![
        column![
            text("计划调度与完成动作")
                .size(22)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            text("定时启停下载队列、分时段自动限速、全部任务完成后执行电源或自定义脚本动作")
                .size(14)
                .font(FONT_REGULAR)
                .color(palette.text_secondary),
        ]
        .spacing(4),
        space::horizontal(),
        button(text("保存调度配置").size(13).font(FONT_MEDIUM))
            .padding([8, 16])
            .style(primary_btn_style(palette))
            .on_press(Message::SchedSave),
    ]
    .align_y(Alignment::Center);

    // Live scheduler status banner
    let status_chip = |label: String, active: bool| {
        container(
            text(label)
                .size(12)
                .font(FONT_MONO)
                .color(if active {
                    palette.accent
                } else {
                    palette.text_muted
                }),
        )
        .padding([4, 10])
        .style(panel_style(
            if active {
                palette.accent_soft
            } else {
                palette.bg_input
            },
            Some(palette.hairline),
            6.0,
        ))
    };

    let status_bar = container(
        row![
            icon(IconKind::Clock, 16.0, palette.accent),
            text(format!("当前系统时间 {now_str}"))
                .size(13)
                .font(FONT_MONO)
                .color(palette.text_primary),
            space::horizontal(),
            status_chip(
                if sched.start_enabled {
                    format!("定时开始 {}", sched.start_time)
                } else {
                    "定时开始 未启用".to_string()
                },
                sched.start_enabled,
            ),
            status_chip(
                if sched.stop_enabled {
                    format!("定时暂停 {}", sched.stop_time)
                } else {
                    "定时暂停 未启用".to_string()
                },
                sched.stop_enabled,
            ),
            status_chip(
                if speed_window_active {
                    format!("时段限速生效中 · {} KB/s", sched.scheduled_speed_limit_kbps)
                } else if sched.speed_schedule_enabled {
                    format!("时段限速待命 {}-{}", sched.speed_start_time, sched.speed_end_time)
                } else {
                    "时段限速 未启用".to_string()
                },
                speed_window_active,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0));

    let start_valid = parse_hhmm(&sched.start_time).is_some();
    let stop_valid = parse_hhmm(&sched.stop_time).is_some();

    let preset_time_btn = |val: &'static str, msg: Message| {
        button(text(val).size(11).font(FONT_MONO))
            .padding([4, 8])
            .style(outline_btn_style(palette))
            .on_press(msg)
    };

    // Card 1: Timed Start / Stop
    let timed_card = container(
        column![
            text("定时队列启停（每日循环）")
                .size(14)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            row![
                column![
                    text("定时自动开始全部暂停/排队任务")
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text("到达指定时刻自动调用「全部继续」，适合夜间闲时宽带拉满")
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted),
                ]
                .spacing(3)
                .width(Length::Fill),
                row![
                    preset_time_btn("00:00", Message::SchedStartTimeChanged("00:00".into())),
                    preset_time_btn("02:00", Message::SchedStartTimeChanged("02:00".into())),
                    preset_time_btn("06:00", Message::SchedStartTimeChanged("06:00".into())),
                    wrap_input(
                        text_input("02:00", &sched.start_time)
                            .on_input(Message::SchedStartTimeChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([6, 10])
                            .width(Length::Fixed(74.0))
                            .style(input_style(palette)),
                        InputTarget::SchedStartTime,
                    ),
                    text(if start_valid { "✓" } else { "格式 HH:MM" })
                        .size(12)
                        .font(FONT_MONO)
                        .color(if start_valid { palette.ok } else { palette.err }),
                    toggler(sched.start_enabled)
                        .on_toggle(Message::SchedToggleStart)
                        .size(18),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            row![
                column![
                    text("定时自动暂停全部活跃下载")
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text("到达指定时刻自动调用「全部暂停」，避免白天占用办公/游戏带宽")
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted),
                ]
                .spacing(3)
                .width(Length::Fill),
                row![
                    preset_time_btn("07:00", Message::SchedStopTimeChanged("07:00".into())),
                    preset_time_btn("08:00", Message::SchedStopTimeChanged("08:00".into())),
                    preset_time_btn("23:30", Message::SchedStopTimeChanged("23:30".into())),
                    wrap_input(
                        text_input("08:00", &sched.stop_time)
                            .on_input(Message::SchedStopTimeChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([6, 10])
                            .width(Length::Fixed(74.0))
                            .style(input_style(palette)),
                        InputTarget::SchedStopTime,
                    ),
                    text(if stop_valid { "✓" } else { "格式 HH:MM" })
                        .size(12)
                        .font(FONT_MONO)
                        .color(if stop_valid { palette.ok } else { palette.err }),
                    toggler(sched.stop_enabled)
                        .on_toggle(Message::SchedToggleStop)
                        .size(18),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(14),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 2: Time-Window Speed Cap
    let speed_start_ok = parse_hhmm(&sched.speed_start_time).is_some();
    let speed_end_ok = parse_hhmm(&sched.speed_end_time).is_some();
    let window_hint = if speed_start_ok && speed_end_ok {
        let now = chrono::Local::now();
        let now_m = (chrono::Timelike::hour(&now) * 60 + chrono::Timelike::minute(&now)) as u16;
        if is_time_in_window(now_m, &sched.speed_start_time, &sched.speed_end_time) {
            "当前处于限速时段内（启用后立即生效）"
        } else {
            "当前处于限速时段外（将使用全局默认限速）"
        }
    } else {
        "请输入有效 HH:MM 时间（支持跨午夜时段，如 22:00 至 07:00）"
    };

    let speed_card = container(
        column![
            row![
                column![
                    text("分时段自动限速")
                        .size(14)
                        .font(FONT_SEMIBOLD)
                        .color(palette.text_primary),
                    text("在指定时间段内自动收紧全局下载速率，离开时段后自动恢复设置页中的全局限速")
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted),
                ]
                .spacing(3)
                .width(Length::Fill),
                toggler(sched.speed_schedule_enabled)
                    .on_toggle(Message::SchedToggleSpeed)
                    .size(18),
            ]
            .align_y(Alignment::Center),
            row![
                text("生效时段")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                wrap_input(
                    text_input("09:00", &sched.speed_start_time)
                        .on_input(Message::SchedSpeedStartChanged)
                        .font(FONT_MONO)
                        .size(13)
                        .padding([6, 10])
                        .width(Length::Fixed(74.0))
                        .style(input_style(palette)),
                    InputTarget::SchedSpeedStart,
                ),
                text("至")
                    .size(13)
                    .font(FONT_REGULAR)
                    .color(palette.text_muted),
                wrap_input(
                    text_input("23:00", &sched.speed_end_time)
                        .on_input(Message::SchedSpeedEndChanged)
                        .font(FONT_MONO)
                        .size(13)
                        .padding([6, 10])
                        .width(Length::Fixed(74.0))
                        .style(input_style(palette)),
                    InputTarget::SchedSpeedEnd,
                ),
                space::horizontal().width(12),
                text("时段上限")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                wrap_input(
                    text_input("2048", speed_kbps_input)
                        .on_input(Message::SchedSpeedKbpsChanged)
                        .font(FONT_MONO)
                        .size(13)
                        .padding([6, 10])
                        .width(Length::Fixed(96.0))
                        .style(input_style(palette)),
                    InputTarget::SchedSpeedKbps,
                ),
                text("KB/s")
                    .size(12)
                    .font(FONT_MONO)
                    .color(palette.text_muted),
                space::horizontal(),
                preset_time_btn("1 MB/s", Message::SchedSpeedKbpsChanged("1024".into())),
                preset_time_btn("5 MB/s", Message::SchedSpeedKbpsChanged("5120".into())),
                preset_time_btn("10 MB/s", Message::SchedSpeedKbpsChanged("10240".into())),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(window_hint)
                .size(12)
                .font(FONT_REGULAR)
                .color(if speed_start_ok && speed_end_ok {
                    palette.text_secondary
                } else {
                    palette.err
                }),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 3: Queue Complete Actions
    let action_btn = |label: &'static str, action: QueueCompleteAction| {
        let active = sched.on_queue_complete == action;
        button(
            text(label)
                .size(13)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR }),
        )
        .padding([7, 13])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::SchedSetCompleteAction(action))
    };

    let mut action_col = column![
        text("队列全部下载完成后自动执行")
            .size(14)
            .font(FONT_SEMIBOLD)
            .color(palette.text_primary),
        text("当所有进行中与排队中的任务均下载完成后自动触发（关机默认预留 30 秒倒计时，随时可用 shutdown /a 取消）")
            .size(12)
            .font(FONT_REGULAR)
            .color(palette.text_muted),
        row![
            action_btn("无动作", QueueCompleteAction::None),
            action_btn("退出 Siphon", QueueCompleteAction::ExitApp),
            action_btn("系统睡眠", QueueCompleteAction::Sleep),
            action_btn("系统休眠", QueueCompleteAction::Hibernate),
            action_btn("30 秒后关机", QueueCompleteAction::Shutdown),
            action_btn("执行自定义脚本", QueueCompleteAction::RunScript),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(12);

    if sched.on_queue_complete == QueueCompleteAction::RunScript {
        action_col = action_col.push(
            column![
                text("自定义 PowerShell 命令 / 脚本路径")
                    .size(12)
                    .font(FONT_MEDIUM)
                    .color(palette.text_secondary),
                wrap_input(
                    text_input(
                        r#"例如: powershell -File "D:\scripts\notify.ps1""#,
                        &sched.post_queue_script,
                    )
                    .on_input(Message::SchedScriptChanged)
                    .font(FONT_MONO)
                    .size(13)
                    .padding([8, 12])
                    .width(Length::Fill)
                    .style(input_style(palette)),
                    InputTarget::SchedScript,
                ),
            ]
            .spacing(6),
        );
    }

    action_col = action_col
        .push(rule::horizontal(1).style(hairline_rule_style(palette)))
        .push(
            row![
                column![
                    text("单次触发保护（执行完成后自动重置为「无动作」）")
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(palette.text_primary),
                    text("防止下次启动下载小文件后意外触发睡眠或关机，强烈建议保持开启")
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted),
                ]
                .spacing(3)
                .width(Length::Fill),
                toggler(sched.one_shot_complete_action)
                    .on_toggle(Message::SchedToggleOneShot)
                    .size(18),
            ]
            .align_y(Alignment::Center),
        );

    let action_card = container(action_col)
        .padding(20)
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    let body = column![header, status_bar, timed_card, speed_card, action_card]
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
