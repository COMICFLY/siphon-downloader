use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_MONO_BOLD, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette,
    clean_scrollable_style, danger_btn_style, hairline_rule_style, input_style, outline_btn_style,
    panel_style, primary_btn_style, subtle_btn_style,
};
use iced::widget::{
    button, column, container, mouse_area, row, rule, scrollable, space, text, text_input, toggler,
};
use iced::{Alignment, Color, Element, Length};
use siphon_core::{BtConfig, BtEncryptionMode, format_speed};
use siphon_engine_bt::{BtPortCheckReport, BtPortLight, BtSessionStatusSnapshot};

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
    bt_status: &'a BtSessionStatusSnapshot,
    bt_port_report: Option<&'a BtPortCheckReport>,
    bt_checking_port: bool,
    listen_port_input: &'a str,
    max_peers_input: &'a str,
    timeout_input: &'a str,
    rw_timeout_input: &'a str,
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
            text("管理 BT / 磁力引擎开关、做种状态与策略、实时端口诊断及 Tracker 订阅")
                .size(14)
                .font(FONT_REGULAR)
                .color(palette.text_secondary),
        ]
        .spacing(4),
        space::horizontal(),
        button(
            text("保存 BT 设置")
                .size(13)
                .font(FONT_MEDIUM)
                .wrapping(text::Wrapping::None),
        )
        .padding([8, 16])
        .style(primary_btn_style(palette))
        .on_press(Message::BtSave),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let stat_chip = |label: String, fg: Color, bg: Color| {
        container(
            text(label)
                .size(12)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::None)
                .color(fg),
        )
        .padding([4, 10])
        .style(panel_style(bg, Some(palette.hairline), 6.0))
    };

    let status_headline = if !bt.enabled {
        "BitTorrent 引擎当前已关闭".to_string()
    } else if bt_status.seeding_torrents > 0 {
        format!(
            "BT 引擎运行中 · 正在做种 {} 个任务（↑ {}）",
            bt_status.seeding_torrents,
            format_speed(bt_status.upload_speed_bps)
        )
    } else if bt.auto_append_trackers && !bt.disable_trackers {
        format!("BT 引擎运行中 · 生效 Tracker：{effective_count} 条")
    } else if bt.disable_trackers {
        "BT 引擎运行中 · 纯 DHT 模式（Tracker 已禁用）".to_string()
    } else {
        "BT 引擎运行中 · 自动注入自定义 Tracker 已关闭".to_string()
    };

    let status_bar = container(
        row![
            icon(
                IconKind::Magnet,
                16.0,
                if !bt.enabled {
                    palette.text_muted
                } else if bt_status.seeding_torrents > 0 {
                    palette.ok
                } else {
                    palette.accent
                }
            ),
            text(status_headline)
                .size(13)
                .font(FONT_MEDIUM)
                .color(if bt.enabled {
                    palette.text_primary
                } else {
                    palette.text_secondary
                }),
            space::horizontal(),
            stat_chip(
                if !bt.enabled {
                    "引擎已关闭".to_string()
                } else if bt_status.seeding_torrents > 0 {
                    format!("做种中 {}", bt_status.seeding_torrents)
                } else if bt.seed_after_complete {
                    "自动做种 开".to_string()
                } else {
                    "自动做种 关".to_string()
                },
                if !bt.enabled {
                    palette.text_muted
                } else if bt_status.seeding_torrents > 0 || bt.seed_after_complete {
                    palette.ok
                } else {
                    palette.text_secondary
                },
                if bt.enabled && (bt_status.seeding_torrents > 0 || bt.seed_after_complete) {
                    palette.ok_soft
                } else {
                    palette.bg_input
                },
            ),
            stat_chip(
                match bt.encryption_mode {
                    BtEncryptionMode::PreferEncrypted => "MSE 优先加密".to_string(),
                    BtEncryptionMode::ForceEncrypted => "MSE 强制加密".to_string(),
                    BtEncryptionMode::Disabled => "明文握手".to_string(),
                },
                if bt.enabled && bt.encryption_mode != BtEncryptionMode::Disabled {
                    palette.accent
                } else {
                    palette.text_secondary
                },
                if bt.enabled && bt.encryption_mode != BtEncryptionMode::Disabled {
                    palette.accent_soft
                } else {
                    palette.bg_input
                },
            ),
            stat_chip(
                if bt.ipv4_only {
                    "仅 IPv4".to_string()
                } else {
                    "双栈".to_string()
                },
                if bt.enabled && !bt.ipv4_only {
                    palette.accent
                } else {
                    palette.text_secondary
                },
                if bt.enabled && !bt.ipv4_only {
                    palette.accent_soft
                } else {
                    palette.bg_input
                },
            ),
            stat_chip(
                format!("DHT {}", if bt.enable_dht { "开" } else { "关" }),
                if bt.enabled && bt.enable_dht {
                    palette.accent
                } else {
                    palette.text_secondary
                },
                if bt.enabled && bt.enable_dht {
                    palette.accent_soft
                } else {
                    palette.bg_input
                },
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 10.0));

    // Card 0: Real-Time Connection Telemetry, Seeding Status & Port Reachability Diagnostics
    let effective_light = match bt_port_report.map(|r| r.light) {
        Some(BtPortLight::Red) => BtPortLight::Red,
        Some(BtPortLight::Green) => BtPortLight::Green,
        Some(BtPortLight::Yellow) if bt_status.incoming_observed => BtPortLight::Green,
        Some(BtPortLight::Yellow) => BtPortLight::Yellow,
        _ if bt_status.incoming_observed => BtPortLight::Green,
        _ => BtPortLight::Unknown,
    };

    let (light_color, light_bg, light_title) = if !bt.enabled {
        (palette.text_muted, palette.bg_input, "已关闭")
    } else if bt_checking_port {
        (palette.accent, palette.accent_soft, "正在探测端口...")
    } else {
        match effective_light {
            BtPortLight::Green => (palette.ok, palette.ok_soft, "入站畅通"),
            BtPortLight::Yellow => (palette.warn, palette.warn_soft, "仅出站受限"),
            BtPortLight::Red => (palette.err, palette.err_soft, "监听异常"),
            BtPortLight::Unknown => (palette.text_secondary, palette.bg_input, "待检测"),
        }
    };

    let bound_addr_label = bt_status
        .listen_addr
        .clone()
        .or_else(|| bt_port_report.map(|r| r.listen_addr.clone()))
        .unwrap_or_else(|| {
            if !bt.enabled {
                "未启动".to_string()
            } else if bt_status.session_ready {
                format!(":{}", bt_status.listen_port)
            } else {
                "预热/未绑定".to_string()
            }
        });

    let metric_tile = |label: &'static str, main_val: String, main_color: Color, sub_val: String| {
        container(
            column![
                text(label)
                    .size(11)
                    .font(FONT_REGULAR)
                    .color(palette.text_muted),
                text(main_val)
                    .size(14)
                    .font(FONT_MONO_BOLD)
                    .color(main_color),
                text(sub_val)
                    .size(11)
                    .font(FONT_MONO)
                    .color(palette.text_secondary),
            ]
            .spacing(3),
        )
        .padding([10, 12])
        .width(Length::Fill)
        .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0))
    };

    let outbound_ok =
        bt_status.tcp_v4_ok + bt_status.tcp_v6_ok + bt_status.utp_v4_ok + bt_status.utp_v6_ok;
    let outbound_err =
        bt_status.tcp_v4_err + bt_status.tcp_v6_err + bt_status.utp_v4_err + bt_status.utp_v6_err;
    let v4_ok = bt_status.tcp_v4_ok + bt_status.utp_v4_ok;
    let v6_ok = bt_status.tcp_v6_ok + bt_status.utp_v6_ok;
    let dht_total = bt_status.dht_nodes_v4 + bt_status.dht_nodes_v6;

    let (seeding_main, seeding_color) = if !bt.enabled {
        ("已关闭".to_string(), palette.text_muted)
    } else if bt_status.seeding_torrents > 0 {
        (format!("做种中 {} 个", bt_status.seeding_torrents), palette.ok)
    } else if bt.seed_after_complete {
        ("待机 · 自动做种".to_string(), palette.text_primary)
    } else {
        ("自动做种已关闭".to_string(), palette.text_secondary)
    };
    let seeding_sub = if !bt.enabled {
        "开启引擎后可用".to_string()
    } else {
        let ratio_hint = if bt.max_seed_ratio > 0.0 {
            format!("上限 {:.1}", bt.max_seed_ratio)
        } else {
            "分享率不限".to_string()
        };
        format!(
            "↑ {} · {}",
            format_speed(bt_status.upload_speed_bps),
            ratio_hint
        )
    };

    let mut live_diag_col = column![
        row![
            column![
                text("实时连接、做种与端口状态")
                    .size(14)
                    .font(FONT_SEMIBOLD)
                    .color(palette.text_primary),
                text(if !bt.enabled {
                    "BitTorrent 引擎已关闭，开启下方总开关后自动监听端口并接入 DHT / Swarm".to_string()
                } else {
                    format!(
                        "监听地址：{bound_addr_label}  ·  ↓ {}  ↑ {}  ·  做种中 {} 个",
                        format_speed(bt_status.download_speed_bps),
                        format_speed(bt_status.upload_speed_bps),
                        bt_status.seeding_torrents,
                    )
                })
                .size(12)
                .font(FONT_MONO)
                .color(palette.text_muted),
            ]
            .spacing(3)
            .width(Length::Fill),
            container(
                row![
                    container(space())
                        .width(Length::Fixed(8.0))
                        .height(Length::Fixed(8.0))
                        .style(panel_style(light_color, None, 4.0)),
                    text(light_title)
                        .size(12)
                        .font(FONT_MEDIUM)
                        .wrapping(text::Wrapping::None)
                        .color(light_color),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([5, 10])
            .style(panel_style(light_bg, Some(palette.hairline), 7.0)),
            button(
                row![
                    icon(IconKind::Refresh, 13.0, palette.cta_text),
                    text(if bt_checking_port {
                        "检测中..."
                    } else {
                        "立即检测端口状态"
                    })
                    .size(12)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([6, 12])
            .style(primary_btn_style(palette))
            .on_press_maybe((bt.enabled && !bt_checking_port).then_some(Message::BtCheckPort)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        row![
            metric_tile("做种状态", seeding_main, seeding_color, seeding_sub),
            metric_tile(
                "全局活跃 Peer",
                format!("{} 个（远端做种 {}）", bt_status.live_peers, bt_status.live_seeders),
                if bt_status.live_peers > 0 {
                    palette.accent
                } else {
                    palette.text_primary
                },
                format!(
                    "TCP {} · uTP {} · 发现 {}",
                    bt_status.live_tcp, bt_status.live_utp, bt_status.seen_peers
                ),
            ),
            metric_tile(
                "DHT 路由表节点",
                format!("{dht_total} 节点"),
                if dht_total > 0 {
                    palette.ok
                } else {
                    palette.text_primary
                },
                format!(
                    "IPv4 {} · IPv6 {}",
                    bt_status.dht_nodes_v4, bt_status.dht_nodes_v6
                ),
            ),
            metric_tile(
                "入站与出站握手",
                if !bt.enabled {
                    "引擎已关闭".to_string()
                } else if bt_status.incoming_observed {
                    "已收到外部连入".to_string()
                } else if outbound_ok > 0 {
                    format!("出站成功 {outbound_ok}")
                } else if bt_status.session_ready {
                    "监听运行中".to_string()
                } else {
                    "预热中".to_string()
                },
                if !bt.enabled {
                    palette.text_muted
                } else if bt_status.incoming_observed || outbound_ok > 0 {
                    palette.ok
                } else if outbound_err > 0 {
                    palette.warn
                } else {
                    palette.text_primary
                },
                format!("v4:{v4_ok} v6:{v6_ok} · 失败 {outbound_err}"),
            ),
        ]
        .spacing(10),
    ]
    .spacing(12);

    if !bt.enabled {
        live_diag_col = live_diag_col.push(
            container(
                text("BitTorrent 引擎当前处于关闭状态，端口监听与 DHT 已释放；开启下方「启用 BitTorrent / 磁力下载引擎」开关即可恢复。")
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary),
            )
            .padding([9, 12])
            .width(Length::Fill)
            .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
        );
    } else if let Some(rep) = bt_port_report {
        let probe_badge = |label: &'static str, val: String, color: Color| {
            container(
                row![
                    text(label)
                        .size(11)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted),
                    text(val).size(11).font(FONT_MONO).color(color),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .style(panel_style(palette.bg_input, Some(palette.hairline), 6.0))
        };

        let ext_val = match rep.external_port_open {
            Some(true) => ("外网 TCP 开放".to_string(), palette.ok),
            Some(false) => ("外网 TCP 阻塞".to_string(), palette.warn),
            None => ("未完成外网探测".to_string(), palette.text_muted),
        };
        let v6_local_val = match rep.local_v6_ok {
            Some(true) => ("正常".to_string(), palette.ok),
            Some(false) => ("未监听".to_string(), palette.warn),
            None => ("已关闭".to_string(), palette.text_muted),
        };
        let pub_v6_val = match &rep.public_ipv6 {
            Some(ip) => {
                let short: String = ip.chars().take(20).collect();
                (format!("可用 ({short})"), palette.ok)
            }
            None => ("无公网 IPv6".to_string(), palette.text_muted),
        };
        let pub_v4_val = match &rep.public_ipv4 {
            Some(ip) => (ip.clone(), palette.text_secondary),
            None => ("未获取".to_string(), palette.text_muted),
        };

        let mut probe_row = row![
            probe_badge(
                "本地 v4",
                if rep.local_v4_ok {
                    "正常".to_string()
                } else {
                    "异常".to_string()
                },
                if rep.local_v4_ok {
                    palette.ok
                } else {
                    palette.err
                },
            ),
            probe_badge("本地 v6", v6_local_val.0, v6_local_val.1),
            probe_badge("外网端口", ext_val.0, ext_val.1),
            probe_badge("公网 v4", pub_v4_val.0, pub_v4_val.1),
            probe_badge("公网 v6", pub_v6_val.0, pub_v6_val.1),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        if let Some(tun) = &rep.detected_tun_adapter {
            probe_row = probe_row.push(probe_badge("TUN", tun.clone(), palette.warn));
        }
        probe_row = probe_row.push(space::horizontal()).push(
            text(format!("检测于 {}", rep.checked_at))
                .size(11)
                .font(FONT_MONO)
                .color(palette.text_muted),
        );

        live_diag_col = live_diag_col.push(probe_row).push(
            container(
                column![
                    text(&rep.summary)
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(light_color),
                    text(&rep.advice)
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                ]
                .spacing(4),
            )
            .padding([10, 14])
            .width(Length::Fill)
            .style(panel_style(light_bg, Some(palette.hairline), 8.0)),
        );
    } else {
        live_diag_col = live_diag_col.push(
            container(
                text("点击右侧「立即检测端口状态」可自动探测本机 IPv4/IPv6 环回监听、公网 IPv6 可用性与外网 TCP 端口开放情况，并给出路由器 / 防火墙优化建议。")
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_secondary),
            )
            .padding([9, 12])
            .width(Length::Fill)
            .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
        );
    }

    let live_diag_card = container(live_diag_col)
        .padding(20)
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

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

    let enc_mode_btn = |label: &'static str, mode: BtEncryptionMode| {
        let active = bt.encryption_mode == mode;
        button(
            text(label)
                .size(12)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR }),
        )
        .padding([5, 10])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::BtSetEncryptionMode(mode))
    };

    let encryption_row = row![
        column![
            text("协议加密模式（MSE/PE · 强烈推荐）")
                .size(13)
                .font(FONT_MEDIUM)
                .color(palette.text_primary),
            text("使用 768-bit DH + RC4 加密 BitTorrent 握手与传输流，可穿透 TUN 代理/运营商明文拦截并兼容要求加密的 Peer")
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
        ]
        .spacing(3)
        .width(Length::Fill),
        container(
            row![
                enc_mode_btn("优先加密", BtEncryptionMode::PreferEncrypted),
                enc_mode_btn("强制加密", BtEncryptionMode::ForceEncrypted),
                enc_mode_btn("关闭明文", BtEncryptionMode::Disabled),
            ]
            .spacing(2),
        )
        .padding(2)
        .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let ratio_btn = |label: &'static str, ratio: f64| {
        let active = (bt.max_seed_ratio - ratio).abs() < 0.05;
        button(
            text(label)
                .size(12)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                .wrapping(text::Wrapping::None),
        )
        .padding([5, 10])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::BtSetMaxSeedRatio(ratio))
    };

    let seed_ratio_row = row![
        column![
            text("做种分享率上限")
                .size(13)
                .font(FONT_MEDIUM)
                .color(palette.text_primary),
            text("达到目标分享率（上传量 ÷ 下载量）后自动结束做种；设为「不限」则保持做种直至手动停止")
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
        ]
        .spacing(3)
        .width(Length::Fill),
        container(
            row![
                ratio_btn("不限", 0.0),
                ratio_btn("1.0", 1.0),
                ratio_btn("2.0", 2.0),
                ratio_btn("3.0", 3.0),
            ]
            .spacing(2),
        )
        .padding(2)
        .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    // Card 1: BT Engine Switches, Seeding Policy & Core Network Settings
    let network_card = container(
        column![
            text("BT 引擎开关、做种策略与网络参数")
                .size(14)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            toggle_row(
                "启用 BitTorrent / 磁力下载引擎",
                "关闭后立即停止 BT 监听端口与 DHT 节点，并不再接收新的磁力链接或 .torrent 任务",
                bt.enabled,
                Message::BtToggleEnabled,
            ),
            toggle_row(
                "下载完成后自动保持做种",
                "开启后，BT / 磁力任务下载至 100% 时自动转为「做种中」状态继续上传回馈 Swarm，且不占用下载并发配额",
                bt.seed_after_complete,
                Message::BtToggleSeedAfterComplete,
            ),
            seed_ratio_row,
            rule::horizontal(1).style(hairline_rule_style(palette)),
            encryption_row,
            rule::horizontal(1).style(hairline_rule_style(palette)),
            toggle_row(
                "自动向每个 BT / 磁力任务追加下方 Tracker",
                "开启后，新建或恢复磁力链接 / .torrent 任务时自动合并注入全部自定义与订阅 Tracker（强烈推荐）",
                bt.auto_append_trackers,
                Message::BtToggleAutoAppend,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            toggle_row(
                "启用 IPv4 + IPv6 双栈连接（强烈推荐）",
                "关闭「仅限 IPv4」，绑定 [::] 双栈套接字直连公网 IPv6 Peer，显著改善大内网 NAT 下的连上率与下载速度",
                !bt.ipv4_only,
                Message::BtToggleIpv6DualStack,
            ),
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
                    text("固定监听端口（0 = 随机）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("16881", listen_port_input)
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
                        text_input("160", max_peers_input)
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
                    text("连接超时（秒）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("12", timeout_input)
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
                column![
                    text("读写空闲超时（秒）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("60", rw_timeout_input)
                            .on_input(Message::BtRwTimeoutChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::BtRwTimeout,
                    ),
                ]
                .spacing(5)
                .width(Length::Fill),
            ]
            .spacing(12),
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

    let body = column![
        header,
        status_bar,
        live_diag_card,
        network_card,
        sub_card,
        custom_card
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
