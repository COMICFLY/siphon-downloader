use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message, ServerTestStatus};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, clean_scrollable_style,
    danger_btn_style, hairline_rule_style, input_style, outline_btn_style, panel_style,
    primary_btn_style,
};
use iced::widget::{
    button, column, container, mouse_area, row, rule, scrollable, space, text, text_input, toggler,
};
use iced::{Alignment, Element, Length};
use siphon_core::Ed2kConfig;
use std::collections::HashMap;

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
    ed2k: &'a Ed2kConfig,
    tcp_port_input: &'a str,
    udp_port_input: &'a str,
    max_sources_input: &'a str,
    upload_limit_input: &'a str,
    quick_server_link_input: &'a str,
    new_srv_name: &'a str,
    new_srv_host: &'a str,
    new_srv_port: &'a str,
    syncing_met: bool,
    met_sync_status: Option<&'a str>,
    server_tests: &'a HashMap<String, ServerTestStatus>,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let enabled_srv_count = ed2k.servers.iter().filter(|s| s.enabled).count();

    let header = row![
        column![
            text("eD2k 电驴协议设置")
                .size(22)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            text("配置 eD2k / Kad 网络端口、MD4 根哈希校验与服务器列表（支持 server.met 同步及节点测速）")
                .size(14)
                .font(FONT_REGULAR)
                .color(palette.text_secondary),
        ]
        .spacing(4),
        space::horizontal(),
        button(text("保存 eD2k 设置").size(13).font(FONT_MEDIUM))
            .padding([8, 16])
            .style(primary_btn_style(palette))
            .on_press(Message::Ed2kSave),
    ]
    .spacing(8)
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
            icon(IconKind::Server, 16.0, palette.accent),
            text(if ed2k.enabled {
                format!(
                    "eD2k 协议已启用 · 已配置 {enabled_srv_count}/{} 台服务器",
                    ed2k.servers.len()
                )
            } else {
                "eD2k 协议当前已关闭".to_string()
            })
            .size(13)
            .font(FONT_MEDIUM)
            .color(palette.text_primary),
            space::horizontal(),
            stat_chip(format!("TCP {}", ed2k.tcp_port), ed2k.enabled),
            stat_chip(
                format!("Kad UDP {}", ed2k.udp_port),
                ed2k.enabled && ed2k.enable_kad,
            ),
            stat_chip(
                format!("MD4 校验 {}", if ed2k.verify_md4_hash { "开" } else { "关" }),
                ed2k.verify_md4_hash,
            ),
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

    // Card 1: Core eD2k Protocol Settings
    let core_card = container(
        column![
            text("eD2k 核心参数与端口")
                .size(14)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            toggle_row(
                "启用 eD2k 电驴协议支持",
                "允许通过快速栏、新建弹窗、剪贴板、拖放与浏览器扩展接管 ed2k://|file|... 链接",
                ed2k.enabled,
                Message::Ed2kToggleEnabled,
            ),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            row![
                column![
                    text("客户端昵称 (Nickname)")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("Siphon", &ed2k.nickname)
                            .on_input(Message::Ed2kNicknameChanged)
                            .font(FONT_REGULAR)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::Ed2kNickname,
                    ),
                ]
                .spacing(5)
                .width(Length::FillPortion(2)),
                column![
                    text("TCP 端口")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("4662", tcp_port_input)
                            .on_input(Message::Ed2kTcpPortChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::Ed2kTcpPort,
                    ),
                ]
                .spacing(5)
                .width(Length::FillPortion(1)),
                column![
                    text("Kad UDP 端口")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("4672", udp_port_input)
                            .on_input(Message::Ed2kUdpPortChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::Ed2kUdpPort,
                    ),
                ]
                .spacing(5)
                .width(Length::FillPortion(1)),
                column![
                    text("单文件最大来源数")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("400", max_sources_input)
                            .on_input(Message::Ed2kMaxSourcesChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::Ed2kMaxSources,
                    ),
                ]
                .spacing(5)
                .width(Length::FillPortion(1)),
                column![
                    text("上传限速 (KB/s)")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("0 为不限", upload_limit_input)
                            .on_input(Message::Ed2kUploadLimitChanged)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::Ed2kUploadLimit,
                    ),
                ]
                .spacing(5)
                .width(Length::FillPortion(1)),
            ]
            .spacing(12),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            toggle_row(
                "启用 Kad 无服务器网络",
                "配合 Kademlia UDP 分布式哈希表检索节点与文件来源",
                ed2k.enable_kad,
                Message::Ed2kToggleKad,
            ),
            toggle_row(
                "启用协议加密混淆",
                "对 eD2k 控制与传输报文启用混淆策略，降低宽带运营商 QoS 限流概率",
                ed2k.enable_obfuscation,
                Message::Ed2kToggleObfuscation,
            ),
            toggle_row(
                "下载完成后强制执行 eD2k MD4 完整性校验",
                "严格按照 9,728,000 字节分块 MD4 哈希树校验下载文件，防止损坏或伪造源（推荐开启）",
                ed2k.verify_md4_hash,
                Message::Ed2kToggleVerifyMd4,
            ),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 2: server.met Sync & Quick Import
    let mut met_col = column![
        row![
            column![
                text("server.met 服务器列表订阅与快捷导入")
                    .size(14)
                    .font(FONT_SEMIBOLD)
                    .color(palette.text_primary),
                text("支持解析标准二进制 server.met 与文本服务器清单，也可直接粘贴 ed2k://|server|IP|PORT|/ 链接")
                    .size(12)
                    .font(FONT_REGULAR)
                    .color(palette.text_muted),
            ]
            .spacing(3)
            .width(Length::Fill),
            button(text("恢复默认服务器").size(12).font(FONT_REGULAR))
                .padding([6, 10])
                .style(outline_btn_style(palette))
                .on_press(Message::Ed2kResetDefaultServers),
        ]
        .align_y(Alignment::Center),
        row![
            wrap_input(
                text_input(
                    "http://upd.emule-security.org/server.met",
                    &ed2k.server_met_url,
                )
                .on_input(Message::Ed2kServerMetUrlChanged)
                .on_submit(Message::Ed2kSyncServerMet)
                .font(FONT_MONO)
                .size(12)
                .padding([8, 10])
                .width(Length::Fill)
                .style(input_style(palette)),
                InputTarget::Ed2kServerMetUrl,
            ),
            button(
                row![
                    icon(IconKind::Refresh, 13.0, palette.cta_text),
                    text(if syncing_met {
                        "同步中..."
                    } else {
                        "同步 server.met"
                    })
                    .size(12)
                    .font(FONT_MEDIUM),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([8, 14])
            .style(primary_btn_style(palette))
            .on_press_maybe((!syncing_met).then_some(Message::Ed2kSyncServerMet)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![
            wrap_input(
                text_input(
                    "快速粘贴 ed2k://|server|45.82.80.155|5687|/ 或 IP:PORT",
                    quick_server_link_input,
                )
                .on_input(Message::Ed2kQuickServerLinkChanged)
                .on_submit(Message::Ed2kImportQuickServerLink)
                .font(FONT_MONO)
                .size(12)
                .padding([8, 10])
                .width(Length::Fill)
                .style(input_style(palette)),
                InputTarget::Ed2kQuickServerLink,
            ),
            button(text("解析并导入").size(12).font(FONT_MEDIUM))
                .padding([8, 14])
                .style(outline_btn_style(palette))
                .on_press_maybe(
                    (!quick_server_link_input.trim().is_empty())
                        .then_some(Message::Ed2kImportQuickServerLink)
                ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(10);

    if let Some(msg) = met_sync_status {
        met_col = met_col.push(
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

    let met_card = container(met_col)
        .padding(20)
        .width(Length::Fill)
        .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Card 3: eD2k Server List + Live TCP Connectivity Test
    let mut srv_list = column![].spacing(8);
    if ed2k.servers.is_empty() {
        srv_list = srv_list.push(
            text("暂无 eD2k 服务器，请使用上方 server.met 同步或手动添加节点")
                .size(12)
                .font(FONT_REGULAR)
                .color(palette.text_muted),
        );
    } else {
        for (idx, srv) in ed2k.servers.iter().enumerate() {
            let key = format!("{}:{}", srv.host, srv.port);
            let test_badge: Element<'a, Message> = match server_tests.get(&key) {
                Some(ServerTestStatus::Testing) => container(
                    text("测速中...")
                        .size(11)
                        .font(FONT_MONO)
                        .color(palette.warn),
                )
                .padding([3, 8])
                .style(panel_style(palette.warn_soft, None, 5.0))
                .into(),
                Some(ServerTestStatus::Online(ms)) => container(
                    text(format!("连通 · {ms} ms"))
                        .size(11)
                        .font(FONT_MONO)
                        .color(palette.ok),
                )
                .padding([3, 8])
                .style(panel_style(palette.ok_soft, None, 5.0))
                .into(),
                Some(ServerTestStatus::Offline(err)) => container(
                    text(format!("不可达 ({err})"))
                        .size(11)
                        .font(FONT_MONO)
                        .color(palette.err),
                )
                .padding([3, 8])
                .style(panel_style(palette.err_soft, None, 5.0))
                .into(),
                None => container(
                    text("未测试")
                        .size(11)
                        .font(FONT_MONO)
                        .color(palette.text_muted),
                )
                .padding([3, 8])
                .style(panel_style(palette.bg_card, Some(palette.hairline), 5.0))
                .into(),
            };

            srv_list = srv_list.push(
                container(
                    row![
                        toggler(srv.enabled)
                            .on_toggle(move |_| Message::Ed2kToggleServerEnabled(idx))
                            .size(16),
                        column![
                            text(&srv.name)
                                .size(13)
                                .font(FONT_MEDIUM)
                                .color(if srv.enabled {
                                    palette.text_primary
                                } else {
                                    palette.text_muted
                                }),
                            text(format!("{}:{}", srv.host, srv.port))
                                .size(12)
                                .font(FONT_MONO)
                                .color(palette.text_secondary),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        test_badge,
                        button(text("连通测试").size(12).font(FONT_REGULAR))
                            .padding([5, 10])
                            .style(outline_btn_style(palette))
                            .on_press(Message::Ed2kTestServer(idx)),
                        button(icon(IconKind::Trash, 13.0, palette.err))
                            .padding([5, 8])
                            .style(danger_btn_style(palette))
                            .on_press(Message::Ed2kRemoveServer(idx)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding([10, 12])
                .width(Length::Fill)
                .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
            );
        }
    }

    let servers_card = container(
        column![
            text(format!("eD2k 服务器节点列表（{} 台）", ed2k.servers.len()))
                .size(14)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            row![
                wrap_input(
                    text_input("服务器名称（可选）", new_srv_name)
                        .on_input(Message::Ed2kNewServerNameChanged)
                        .font(FONT_REGULAR)
                        .size(12)
                        .padding([7, 10])
                        .width(Length::FillPortion(2))
                        .style(input_style(palette)),
                    InputTarget::Ed2kNewServerName,
                ),
                wrap_input(
                    text_input("主机或 IPv4（如 45.82.80.155）", new_srv_host)
                        .on_input(Message::Ed2kNewServerHostChanged)
                        .font(FONT_MONO)
                        .size(12)
                        .padding([7, 10])
                        .width(Length::FillPortion(3))
                        .style(input_style(palette)),
                    InputTarget::Ed2kNewServerHost,
                ),
                wrap_input(
                    text_input("端口", new_srv_port)
                        .on_input(Message::Ed2kNewServerPortChanged)
                        .on_submit(Message::Ed2kAddServer)
                        .font(FONT_MONO)
                        .size(12)
                        .padding([7, 10])
                        .width(Length::Fixed(84.0))
                        .style(input_style(palette)),
                    InputTarget::Ed2kNewServerPort,
                ),
                button(
                    row![
                        icon(IconKind::Plus, 13.0, palette.text_primary),
                        text("添加服务器").size(12).font(FONT_MEDIUM),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([7, 12])
                .style(outline_btn_style(palette))
                .on_press_maybe(
                    (!new_srv_host.trim().is_empty() && !new_srv_port.trim().is_empty())
                        .then_some(Message::Ed2kAddServer)
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            srv_list,
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    let body = column![header, status_bar, core_card, met_card, servers_card]
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
