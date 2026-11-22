use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, clean_scrollable_style,
    danger_btn_style, hairline_rule_style, input_style, outline_btn_style, panel_style,
    primary_btn_style,
};
use iced::widget::{
    button, column, container, mouse_area, row, rule, scrollable, space, text, text_input, toggler,
};
use iced::{Alignment, Element, Length};
use siphon_core::BtConfig;

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
    bt: &'a BtConfig,
    listen_port_input: &'a str,
    max_peers_input: &'a str,
    timeout_input: &'a str,
    new_tracker_input: &'a str,
    new_sub_url_input: &'a str,
    syncing_trackers: bool,
    sync_status: Option<&'a str>,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let effective_count = bt.effective_trackers().len();

    let header = row![
        column![
            text("BitTorrent 与自定义 Tracker")
                .size(22)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            text("配置 BT / 磁力网络参数、自定义 Tracker 列表以及公共 Tracker 订阅源一键同步")
                .size(14)
                .font(FONT_REGULAR)
                .color(palette.text_secondary),
        ]
        .spacing(4),
        space::horizontal(),
        button(text("保存 BT 设置").size(13).font(FONT_MEDIUM))
            .padding([8, 16])
            .style(primary_btn_style(palette))
            .on_press(Message::BtSave),
    ]
    .align_y(Alignment::Center);

    let stat_chip = |label: String, highlight: bool| {
        container(
            text(label)
                .size(12)
                .font(FONT_MONO)
                .color(if highlight {
                    palette.accent
                } else {
                    palette.text_secondary
                }),
        )
        .padding([4, 10])
        .style(panel_style(
            if highlight {
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
            icon(IconKind::Magnet, 16.0, palette.accent),
            text(if bt.auto_append_trackers && !bt.disable_trackers {
                format!("合并去重生效 Tracker：{effective_count} 条")
            } else if bt.disable_trackers {
                "纯 DHT 模式（Tracker 已禁用）".to_string()
            } else {
                "自动注入自定义 Tracker 已关闭".to_string()
            })
            .size(13)
            .font(FONT_MEDIUM)
            .color(palette.text_primary),
            space::horizontal(),
            stat_chip(
                format!("DHT {}", if bt.enable_dht { "开" } else { "关" }),
                bt.enable_dht,
            ),
            stat_chip(format!("自定义 {}", bt.custom_trackers.len()), false),
            stat_chip(format!("订阅同步 {}", bt.synced_trackers.len()), false),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0));

    let toggle_row = |title: &'static str, desc: &'static str, val: bool, on_toggle: fn(bool) -> Message| {
        row![
            column![
                text(title)
                    .size(13)
                    .font(FONT_MEDIUM)
                    .color(palette.text_primary),
                text(desc)
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_muted),
            ]
            .spacing(3)
            .width(Length::Fill),
            toggler(val).on_toggle(on_toggle).size(18),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
    };

    // Card 1: BT Core Network Settings
    let network_card = container(
        column![
            text("BitTorrent 协议与网络参数")
                .size(14)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            toggle_row(
                "自动向每个 BT / 磁力任务追加下方 Tracker",
                "开启后，新建或恢复磁力链接 / .torrent 任务时自动合并注入全部自定义与订阅 Tracker（强烈推荐）",
                bt.auto_append_trackers,
                Message::BtToggleAutoAppend,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            toggle_row(
                "启用 DHT 分布式网络",
                "无 Tracker 时通过去中心化 DHT 节点网络自动发现 Peer 与种子元数据",
                bt.enable_dht,
                Message::BtToggleDht,
            ),
            toggle_row(
                "启用 LSD 局域网节点发现",
                "在本地局域网内通过组播发现同网段 Peer 加速传输",
                bt.enable_lsd,
                Message::BtToggleLsd,
            ),
            toggle_row(
                "启用 UPnP 自动端口映射",
                "自动向家用路由器申请端口转发，提升外网 Peer 主动连入成功率",
                bt.enable_upnp,
                Message::BtToggleUpnp,
            ),
            toggle_row(
                "禁用全部 Tracker（纯 DHT 模式）",
                "勾选后不向任何 Tracker 发起 announce 请求，仅依赖 DHT 发现节点",
                bt.disable_trackers,
                Message::BtToggleDisableTrackers,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            row![
                column![
                    text("监听端口（0 为随机）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("0", listen_port_input)
                            .on_input(Message::BtListenPortChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::BtListenPort,
                    ),
                ]
                .spacing(5)
                .width(Length::Fill),
                column![
                    text("单任务最大 Peer 数")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("128", max_peers_input)
                            .on_input(Message::BtMaxPeersChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::BtMaxPeers,
                    ),
                ]
                .spacing(5)
                .width(Length::Fill),
                column![
                    text("节点连接超时（秒）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("10", timeout_input)
                            .on_input(Message::BtConnectTimeoutChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::BtConnectTimeout,
                    ),
                ]
                .spacing(5)
                .width(Length::Fill),
            ]
            .spacing(14),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 2: Online Tracker Subscriptions
    let mut sub_list = column![].spacing(6);
    if bt.tracker_subscribe_urls.is_empty() {
        sub_list = sub_list.push(
            text("暂无订阅源，点击右侧「恢复默认订阅源」或手动添加 URL")
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
        );
    } else {
        for (idx, sub_url) in bt.tracker_subscribe_urls.iter().enumerate() {
            sub_list = sub_list.push(
                container(
                    row![
                        text(sub_url)
                            .size(12)
                            .font(FONT_MONO)
                            .color(palette.text_secondary)
                            .width(Length::Fill),
                        button(icon(IconKind::Trash, 13.0, palette.err))
                            .padding([4, 7])
                            .style(danger_btn_style(palette))
                            .on_press(Message::BtRemoveSubscribeUrl(idx)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([7, 10])
                .width(Length::Fill)
                .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0)),
            );
        }
    }

    let mut sub_col = column![
        row![
            column![
                text("公共 Tracker 订阅源一键同步")
                    .size(14)
                    .font(FONT_SEMIBOLD)
                    .color(palette.text_primary),
                text(match &bt.last_tracker_sync_at {
                    Some(ts) => format!(
                        "上次同步：{ts} · 已缓存 {} 条公共 Tracker（自动去重合并）",
                        bt.synced_trackers.len()
                    ),
                    None => format!(
                        "从公开 Tracker 清单自动拉取活跃节点 · 当前已缓存 {} 条",
                        bt.synced_trackers.len()
                    ),
                })
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
            ]
            .spacing(3)
            .width(Length::Fill),
            button(text("恢复默认源").size(12).font(FONT_REGULAR))
                .padding([6, 10])
                .style(outline_btn_style(palette))
                .on_press(Message::BtResetDefaultSubscribeUrls),
            button(
                row![
                    icon(IconKind::Refresh, 13.0, palette.cta_text),
                    text(if syncing_trackers {
                        "同步中..."
                    } else {
                        "立即同步 Tracker"
                    })
                    .size(12)
                    .font(FONT_MEDIUM),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([6, 13])
            .style(primary_btn_style(palette))
            .on_press_maybe((!syncing_trackers).then_some(Message::BtSyncTrackers)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![
            wrap_input(
                text_input(
                    "添加订阅源 URL（如 https://cf.trackerslist.com/best.txt）",
                    new_sub_url_input,
                )
                .on_input(Message::BtNewSubscribeUrlChanged)
                .on_submit(Message::BtAddSubscribeUrl)
                .font(FONT_MONO)
                .size(12)
                .padding([7, 10])
                .width(Length::Fill)
                .style(input_style(palette)),
                InputTarget::BtNewSubscribeUrl,
            ),
            button(text("添加订阅源").size(12).font(FONT_MEDIUM))
                .padding([7, 12])
                .style(outline_btn_style(palette))
                .on_press_maybe(
                    (!new_sub_url_input.trim().is_empty()).then_some(Message::BtAddSubscribeUrl)
                ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        sub_list,
    ]
    .spacing(10);

    if let Some(msg) = sync_status {
        sub_col = sub_col.push(
            container(
                text(msg)
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary),
            )
            .padding([8, 12])
            .width(Length::Fill)
            .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0)),
        );
    }

    sub_col = sub_col
        .push(rule::horizontal(1).style(hairline_rule_style(palette)))
        .push(
            row![
                text("启动 Siphon 时自动刷新公共 Tracker 订阅源")
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary),
                toggler(bt.auto_sync_trackers_on_start)
                    .on_toggle(Message::BtToggleAutoSyncOnStart)
                    .size(16),
                space::horizontal(),
                button(text("清空已同步缓存").size(12).font(FONT_REGULAR))
                    .padding([5, 10])
                    .style(outline_btn_style(palette))
                    .on_press_maybe(
                        (!bt.synced_trackers.is_empty()).then_some(Message::BtClearSyncedTrackers)
                    ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );

    let sub_card = container(sub_col)
        .padding(20)
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 3: Custom Tracker List Management
    let mut custom_list = column![].spacing(6);
    if bt.custom_trackers.is_empty() {
        custom_list = custom_list.push(
            text("暂无自定义 Tracker，可在上方输入单条或多条（空格/逗号分隔）或从剪贴板一键导入")
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
        );
    } else {
        for (idx, tr) in bt.custom_trackers.iter().enumerate() {
            custom_list = custom_list.push(
                container(
                    row![
                        text(format!("{:>2}.", idx + 1))
                            .size(11)
                            .font(FONT_MONO)
                            .color(palette.text_muted),
                        text(tr)
                            .size(12)
                            .font(FONT_MONO)
                            .color(palette.text_primary)
                            .width(Length::Fill),
                        button(icon(IconKind::Close, 12.0, palette.text_secondary))
                            .padding([4, 7])
                            .style(outline_btn_style(palette))
                            .on_press(Message::BtRemoveCustomTracker(idx)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0)),
            );
        }
    }

    let custom_card = container(
        column![
            row![
                column![
                    text(format!("自定义 Tracker 列表（{} 条）", bt.custom_trackers.len()))
                        .size(14)
                        .font(FONT_SEMIBOLD)
                        .color(palette.text_primary),
                    text("支持 udp://、http://、https://、ws://、wss:// 协议，支持批量粘贴自动去重")
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted),
                ]
                .spacing(3)
                .width(Length::Fill),
                button(text("粘贴剪贴板批量导入").size(12).font(FONT_REGULAR))
                    .padding([6, 10])
                    .style(outline_btn_style(palette))
                    .on_press(Message::BtPasteTrackersClipboard),
                button(text("清空自定义").size(12).font(FONT_REGULAR))
                    .padding([6, 10])
                    .style(danger_btn_style(palette))
                    .on_press_maybe(
                        (!bt.custom_trackers.is_empty()).then_some(Message::BtClearCustomTrackers)
                    ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                wrap_input(
                    text_input(
                        "udp://tracker.opentrackr.org:1337/announce（可逗号或空格分隔多条）",
                        new_tracker_input,
                    )
                    .on_input(Message::BtNewTrackerChanged)
                    .on_submit(Message::BtAddTracker)
                    .font(FONT_MONO)
                    .size(12)
                    .padding([8, 10])
                    .width(Length::Fill)
                    .style(input_style(palette)),
                    InputTarget::BtNewTracker,
                ),
                button(
                    row![
                        icon(IconKind::Plus, 13.0, palette.text_primary),
                        text("添加 Tracker").size(12).font(FONT_MEDIUM),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .style(outline_btn_style(palette))
                .on_press_maybe(
                    (!new_tracker_input.trim().is_empty()).then_some(Message::BtAddTracker)
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            custom_list,
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    let body = column![header, status_bar, network_card, sub_card, custom_card]
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
