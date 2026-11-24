use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message, RssDraft};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_MONO_BOLD, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette,
    clean_scrollable_style, danger_btn_style, hairline_rule_style, input_style, outline_btn_style,
    panel_style, primary_btn_style, subtle_btn_style,
};
use iced::widget::{
    button, column, container, mouse_area, row, rule, scrollable, space, text, text_input, toggler,
};
use iced::{Alignment, Color, Element, Length};
use siphon_core::{RssConfig, RssFeed, RssItem, RssLinkKind, format_bytes};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub fn rss_url_input_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-rss-url")
}

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
    rss: &'a RssConfig,
    draft: &'a RssDraft,
    selected_feed_id: Option<Uuid>,
    items_by_feed: &'a HashMap<Uuid, Vec<RssItem>>,
    refreshing_feeds: &'a HashSet<Uuid>,
    search: &'a str,
    match_only: bool,
    toast: Option<&'a str>,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let any_refreshing = !refreshing_feeds.is_empty();
    let total_items: usize = rss
        .feeds
        .iter()
        .map(|f| items_by_feed.get(&f.id).map(|v| v.len()).unwrap_or(0))
        .sum();
    let auto_feed_count = rss
        .feeds
        .iter()
        .filter(|f| f.enabled && f.auto_download)
        .count();

    let global_interval_btn = |label: &'static str, mins: u32| {
        let active = rss.default_refresh_interval_mins == mins;
        button(
            text(label)
                .size(11)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                .wrapping(text::Wrapping::None),
        )
        .padding([4, 8])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::RssSetDefaultInterval(mins))
    };

    let header = row![
        column![
            text("RSS 订阅与自动追更")
                .size(22)
                .font(FONT_SEMIBOLD)
                .color(palette.text_primary),
            text("订阅动漫 / BT / 软件发布源，支持关键词与正则过滤、后台定时轮询与自动去重下载")
                .size(14)
                .font(FONT_REGULAR)
                .color(palette.text_secondary),
        ]
        .spacing(4)
        .width(Length::Fill),
        row![
            text("定时刷新")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None)
                .color(palette.text_secondary),
            toggler(rss.enabled)
                .on_toggle(Message::RssToggleGlobalEnabled)
                .size(17),
            container(
                row![
                    global_interval_btn("15分", 15),
                    global_interval_btn("30分", 30),
                    global_interval_btn("1时", 60),
                    global_interval_btn("3时", 180),
                ]
                .spacing(2),
            )
            .padding(2)
            .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0)),
            button(
                row![
                    icon(IconKind::Refresh, 13.0, palette.cta_text),
                    text(if any_refreshing {
                        "刷新中..."
                    } else {
                        "刷新全部"
                    })
                    .size(13)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([7, 14])
            .style(primary_btn_style(palette))
            .on_press_maybe((!any_refreshing && !rss.feeds.is_empty()).then_some(Message::RssRefreshAll)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    // Card 1: Add / Edit RSS Feed Subscription
    let is_editing = draft.editing_id.is_some();
    let can_submit = !draft.url.trim().is_empty();

    let mut form_header = row![
        icon(IconKind::Rss, 15.0, palette.accent),
        text(if is_editing {
            "编辑 RSS 订阅源"
        } else {
            "添加 RSS 订阅源"
        })
        .size(14)
        .font(FONT_SEMIBOLD)
        .color(palette.text_primary),
        space::horizontal(),
        text(format!(
            "已订阅 {} 个源 · 自动追更 {} 个 · 缓存 {} 条",
            rss.feeds.len(),
            auto_feed_count,
            total_items
        ))
        .size(11)
        .font(FONT_MONO)
        .color(palette.text_muted),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if is_editing {
        form_header = form_header.push(
            button(
                text("取消编辑")
                    .size(12)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None),
            )
            .padding([4, 10])
            .style(outline_btn_style(palette))
            .on_press(Message::RssEditCancel),
        );
    }

    let draft_interval_btn = |label: &'static str, mins: u32| {
        let active = draft.refresh_interval_mins == mins;
        button(
            text(label)
                .size(11)
                .font(if active { FONT_MEDIUM } else { FONT_REGULAR })
                .wrapping(text::Wrapping::None),
        )
        .padding([4, 8])
        .style(subtle_btn_style(palette, active))
        .on_press(Message::RssDraftSetInterval(mins))
    };

    let form_card = container(
        column![
            form_header,
            row![
                column![
                    text("订阅地址 (RSS 2.0 / Atom 1.0 URL)")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input(
                            "https://mikanani.me/RSS/... 或任意 RSS/Atom XML 链接",
                            &draft.url,
                        )
                        .id(rss_url_input_id())
                        .on_input(Message::RssDraftUrl)
                        .on_submit(Message::RssDraftSubmit)
                        .font(FONT_MONO)
                        .size(12)
                        .padding([7, 10])
                        .width(Length::Fill)
                        .style(input_style(palette)),
                        InputTarget::RssUrl,
                    ),
                ]
                .spacing(5)
                .width(Length::FillPortion(3)),
                column![
                    text("订阅名称（留空自动读取源标题）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("例如：十月新番追更", &draft.name)
                            .on_input(Message::RssDraftName)
                            .on_submit(Message::RssDraftSubmit)
                            .font(FONT_REGULAR)
                            .size(12)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::RssName,
                    ),
                ]
                .spacing(5)
                .width(Length::FillPortion(2)),
            ]
            .spacing(10),
            row![
                column![
                    text("包含过滤（空格=且，|=或，/正则/）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("留空匹配全部，如 1080p 简日 | 1080p CHS", &draft.include_filter)
                            .on_input(Message::RssDraftInclude)
                            .font(FONT_MONO)
                            .size(12)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::RssInclude,
                    ),
                ]
                .spacing(5)
                .width(Length::Fill),
                column![
                    text("排除过滤（命中即忽略）")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    wrap_input(
                        text_input("可选，如 720p | 繁日 | HEVC", &draft.exclude_filter)
                            .on_input(Message::RssDraftExclude)
                            .font(FONT_MONO)
                            .size(12)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::RssExclude,
                    ),
                ]
                .spacing(5)
                .width(Length::Fill),
            ]
            .spacing(10),
            row![
                row![
                    wrap_input(
                        text_input("自定义保存目录（留空使用默认目录或规则）", &draft.save_dir)
                            .on_input(Message::RssDraftSaveDir)
                            .font(FONT_MONO)
                            .size(12)
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(input_style(palette)),
                        InputTarget::RssSaveDir,
                    ),
                    button(
                        row![
                            icon(IconKind::Folder, 13.0, palette.text_secondary),
                            text("浏览")
                                .size(12)
                                .font(FONT_REGULAR)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([7, 10])
                    .style(outline_btn_style(palette))
                    .on_press(Message::RssDraftBrowseDir),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .width(Length::Fill),
                container(
                    row![
                        draft_interval_btn("跟随全局", 0),
                        draft_interval_btn("15分", 15),
                        draft_interval_btn("30分", 30),
                        draft_interval_btn("1时", 60),
                    ]
                    .spacing(2),
                )
                .padding(2)
                .style(panel_style(palette.bg_input, Some(palette.hairline), 7.0)),
                row![
                    text("自动下载匹配项")
                        .size(12)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_secondary),
                    toggler(draft.auto_download)
                        .on_toggle(Message::RssDraftToggleAutoDownload)
                        .size(16),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                button(
                    row![
                        icon(
                            if is_editing {
                                IconKind::Edit
                            } else {
                                IconKind::Plus
                            },
                            13.0,
                            palette.cta_text,
                        ),
                        text(if is_editing { "保存修改" } else { "添加订阅" })
                            .size(13)
                            .font(FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([7, 14])
                .style(primary_btn_style(palette))
                .on_press_maybe(can_submit.then_some(Message::RssDraftSubmit)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(10),
    )
    .padding(16)
    .width(Length::Fill)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Left column: Feed list
    let all_selected = selected_feed_id.is_none();
    let all_feeds_btn = button(
        row![
            icon(
                IconKind::Rss,
                14.0,
                if all_selected {
                    palette.accent
                } else {
                    palette.text_muted
                }
            ),
            text("全部订阅源")
                .size(13)
                .font(if all_selected {
                    FONT_MEDIUM
                } else {
                    FONT_REGULAR
                })
                .color(palette.text_primary)
                .width(Length::Fill),
            text(format!("{total_items}"))
                .size(11)
                .font(FONT_MONO)
                .color(palette.text_muted),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([9, 12])
    .style(subtle_btn_style(palette, all_selected))
    .on_press(Message::RssSelectFeed(None));

    let mut feed_list_col = column![all_feeds_btn].spacing(6);
    if rss.feeds.is_empty() {
        feed_list_col = feed_list_col.push(
            container(
                column![
                    text("暂无订阅源")
                        .size(13)
                        .font(FONT_MEDIUM)
                        .color(palette.text_secondary),
                    text("在上方粘贴 RSS / Atom 链接添加第一个订阅，支持自动过滤与追更下载")
                        .size(12)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted),
                ]
                .spacing(4),
            )
            .padding(12)
            .width(Length::Fill)
            .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0)),
        );
    } else {
        for feed in &rss.feeds {
            let is_sel = selected_feed_id == Some(feed.id);
            let is_ref = refreshing_feeds.contains(&feed.id);
            let count = items_by_feed.get(&feed.id).map(|v| v.len()).unwrap_or(0);
            feed_list_col = feed_list_col.push(feed_sidebar_card(
                feed,
                is_sel,
                is_ref,
                count,
                rss.default_refresh_interval_mins,
                palette,
            ));
        }
    }

    let feeds_pane = container(
        column![
            row![
                text("订阅源列表")
                    .size(13)
                    .font(FONT_SEMIBOLD)
                    .color(palette.text_primary),
                space::horizontal(),
                text(format!("{} 个", rss.feeds.len()))
                    .size(11)
                    .font(FONT_MONO)
                    .color(palette.text_muted),
            ]
            .align_y(Alignment::Center),
            rule::horizontal(1).style(hairline_rule_style(palette)),
            scrollable(feed_list_col)
                .height(Length::Fill)
                .style(clean_scrollable_style(palette)),
        ]
        .spacing(10),
    )
    .width(Length::Fixed(300.0))
    .height(Length::Fill)
    .padding(14)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    // Right column: Items list for selected feed or all feeds
    let search_low = search.trim().to_ascii_lowercase();
    let selected_feed = selected_feed_id.and_then(|id| rss.feeds.iter().find(|f| f.id == id));

    let mut flat_items: Vec<(&RssFeed, &RssItem, bool, bool)> = Vec::new();
    let target_feeds: Vec<&RssFeed> = match selected_feed {
        Some(f) => vec![f],
        None => rss.feeds.iter().collect(),
    };
    let mut matching_undownloaded = 0usize;

    for feed in &target_feeds {
        if let Some(items) = items_by_feed.get(&feed.id) {
            for item in items {
                let matched = feed.matches_title(&item.title);
                let downloaded = feed.has_downloaded(&item.guid);
                if matched && !downloaded {
                    matching_undownloaded += 1;
                }
                if match_only && !matched {
                    continue;
                }
                if !search_low.is_empty()
                    && !item.title.to_ascii_lowercase().contains(&search_low)
                    && !feed.display_name().to_ascii_lowercase().contains(&search_low)
                {
                    continue;
                }
                flat_items.push((feed, item, matched, downloaded));
            }
        }
    }

    let right_title = match selected_feed {
        Some(f) => f.display_name().to_string(),
        None => "全部订阅内容".to_string(),
    };

    let mut right_actions = row![
        wrap_input(
            text_input("搜索标题...", search)
                .on_input(Message::RssSearchChanged)
                .font(FONT_REGULAR)
                .size(12)
                .padding([5, 9])
                .width(Length::Fixed(160.0))
                .style(input_style(palette)),
            InputTarget::RssSearch,
        ),
        row![
            text("仅看匹配")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None)
                .color(palette.text_secondary),
            toggler(match_only)
                .on_toggle(Message::RssToggleMatchOnly)
                .size(16),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
        button(
            text(if matching_undownloaded > 0 {
                format!("下载匹配 ({matching_undownloaded})")
            } else {
                "下载匹配".to_string()
            })
            .size(12)
            .font(FONT_MEDIUM)
            .wrapping(text::Wrapping::None),
        )
        .padding([5, 10])
        .style(outline_btn_style(palette))
        .on_press_maybe(
            (matching_undownloaded > 0).then_some(Message::RssDownloadAllMatching(selected_feed_id))
        ),
        button(
            text("标为已读")
                .size(12)
                .font(FONT_REGULAR)
                .wrapping(text::Wrapping::None),
        )
        .padding([5, 10])
        .style(outline_btn_style(palette))
        .on_press_maybe(
            (!flat_items.is_empty()).then_some(Message::RssMarkAllRead(selected_feed_id))
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if let Some(f) = selected_feed {
        let is_ref = refreshing_feeds.contains(&f.id);
        let fid = f.id;
        right_actions = right_actions
            .push(
                row![
                    text("自动追更")
                        .size(12)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_secondary),
                    toggler(f.auto_download)
                        .on_toggle(move |v| Message::RssToggleFeedAutoDownload(fid, v))
                        .size(16),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .push(
                button(icon(IconKind::Refresh, 13.0, palette.text_primary))
                    .padding([5, 8])
                    .style(outline_btn_style(palette))
                    .on_press_maybe((!is_ref).then_some(Message::RssRefreshFeed(fid))),
            );
    }

    let mut items_col = column![].spacing(6);
    if flat_items.is_empty() {
        let hint = if rss.feeds.is_empty() {
            "添加 RSS 订阅源并刷新后，最新发布的磁力、种子或直链资源会显示在这里"
        } else if any_refreshing {
            "正在拉取订阅内容，请稍候..."
        } else if match_only {
            "当前过滤条件下没有匹配项，可关闭「仅看匹配」查看全部条目"
        } else {
            "当前订阅源暂无缓存条目，点击「刷新」拉取最新内容"
        };
        items_col = items_col.push(
            container(
                column![
                    icon(IconKind::Rss, 28.0, palette.text_muted),
                    text(hint)
                        .size(13)
                        .font(FONT_REGULAR)
                        .color(palette.text_muted),
                ]
                .spacing(8)
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([48, 24])
            .center_x(Length::Fill),
        );
    } else {
        for (feed, item, matched, downloaded) in flat_items.iter().take(150) {
            items_col = items_col.push(rss_item_row(
                feed,
                item,
                *matched,
                *downloaded,
                selected_feed.is_none(),
                palette,
            ));
        }
    }

    let items_pane = container(
        column![
            row![
                container(
                    text(right_title)
                        .size(14)
                        .font(FONT_SEMIBOLD)
                        .wrapping(text::Wrapping::None)
                        .color(palette.text_primary),
                )
                .width(Length::Fill)
                .clip(true),
                text(format!("{} 条", flat_items.len()))
                    .size(11)
                    .font(FONT_MONO)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            right_actions,
            rule::horizontal(1).style(hairline_rule_style(palette)),
            scrollable(items_col)
                .height(Length::Fill)
                .style(clean_scrollable_style(palette)),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(14)
    .style(panel_style(palette.bg_card, Some(palette.hairline), 12.0));

    let mut page_col = column![header].spacing(14);
    if let Some(msg) = toast {
        page_col = page_col.push(
            container(
                row![
                    text(msg)
                        .size(13)
                        .font(FONT_REGULAR)
                        .color(palette.text_primary),
                    space::horizontal(),
                    button(icon(IconKind::Close, 12.0, palette.text_secondary))
                        .padding([4, 6])
                        .style(subtle_btn_style(palette, false))
                        .on_press(Message::ClearToast),
                ]
                .align_y(Alignment::Center),
            )
            .padding([9, 14])
            .width(Length::Fill)
            .style(panel_style(palette.bg_card, Some(palette.hairline_strong), 8.0)),
        );
    }

    page_col = page_col
        .push(form_card)
        .push(
            row![feeds_pane, items_pane]
                .spacing(14)
                .height(Length::Fill),
        )
        .padding([22, 28])
        .height(Length::Fill);

    container(page_col)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(panel_style(palette.bg_main, None, 0.0))
        .into()
}

fn feed_sidebar_card<'a>(
    feed: &'a RssFeed,
    selected: bool,
    refreshing: bool,
    item_count: usize,
    global_interval: u32,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let dot_color = if !feed.enabled {
        palette.text_muted
    } else if refreshing {
        palette.accent
    } else if feed.last_error.is_some() {
        palette.err
    } else {
        palette.ok
    };

    let interval = feed.effective_interval_mins(global_interval);
    let sub_text = if refreshing {
        "正在刷新...".to_string()
    } else if let Some(err) = &feed.last_error {
        format!("失败: {}", err.chars().take(28).collect::<String>())
    } else if let Some(ts) = &feed.last_refreshed_at {
        format!("{ts} · {interval}m · {item_count} 条")
    } else {
        format!("未刷新 · 每 {interval} 分钟")
    };

    let mut top_row = row![
        container(space())
            .width(Length::Fixed(7.0))
            .height(Length::Fixed(7.0))
            .style(panel_style(dot_color, None, 4.0)),
        container(
            text(feed.display_name())
                .size(13)
                .font(if selected { FONT_MEDIUM } else { FONT_REGULAR })
                .wrapping(text::Wrapping::None)
                .color(if feed.enabled {
                    palette.text_primary
                } else {
                    palette.text_muted
                }),
        )
        .width(Length::Fill)
        .clip(true),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if feed.auto_download {
        top_row = top_row.push(
            container(
                text("自动")
                    .size(10)
                    .font(FONT_MONO_BOLD)
                    .wrapping(text::Wrapping::None)
                    .color(palette.ok),
            )
            .padding([1, 5])
            .style(panel_style(palette.ok_soft, None, 4.0)),
        );
    }

    let feed_id = feed.id;
    let controls_row = row![
        text(sub_text)
            .size(10)
            .font(FONT_MONO)
            .wrapping(text::Wrapping::None)
            .color(if feed.last_error.is_some() {
                palette.err
            } else {
                palette.text_muted
            })
            .width(Length::Fill),
        button(icon(IconKind::Refresh, 11.0, palette.text_secondary))
            .padding([3, 5])
            .style(subtle_btn_style(palette, false))
            .on_press_maybe((!refreshing).then_some(Message::RssRefreshFeed(feed_id))),
        button(icon(IconKind::Edit, 11.0, palette.text_secondary))
            .padding([3, 5])
            .style(subtle_btn_style(palette, false))
            .on_press(Message::RssEditStart(feed_id)),
        toggler(feed.enabled)
            .on_toggle(move |v| Message::RssToggleFeedEnabled(feed_id, v))
            .size(14),
        button(icon(IconKind::Trash, 11.0, palette.err))
            .padding([3, 5])
            .style(danger_btn_style(palette))
            .on_press(Message::RssDeleteFeed(feed_id)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let card_inner = column![
        button(top_row)
            .width(Length::Fill)
            .padding([2, 0])
            .style(subtle_btn_style(palette, false))
            .on_press(Message::RssSelectFeed(Some(feed_id))),
        controls_row,
    ]
    .spacing(6);

    container(card_inner)
        .width(Length::Fill)
        .padding([8, 10])
        .style(panel_style(
            if selected {
                palette.bg_raised
            } else {
                palette.bg_input
            },
            Some(if selected {
                palette.accent
            } else {
                palette.hairline
            }),
            8.0,
        ))
        .into()
}

fn rss_item_row<'a>(
    feed: &'a RssFeed,
    item: &'a RssItem,
    matched: bool,
    downloaded: bool,
    show_feed_name: bool,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let (badge_fg, badge_bg): (Color, Color) = match item.kind {
        RssLinkKind::Magnet => (palette.accent, palette.accent_soft),
        RssLinkKind::Torrent => (palette.warn, palette.warn_soft),
        RssLinkKind::Ed2k => (palette.accent, palette.accent_soft),
        RssLinkKind::Http => (palette.text_secondary, palette.bg_card),
    };

    let mut meta_row = row![
        container(
            text(item.kind.badge())
                .size(10)
                .font(FONT_MONO_BOLD)
                .wrapping(text::Wrapping::None)
                .color(badge_fg),
        )
        .padding([1, 5])
        .style(panel_style(badge_bg, None, 4.0)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if downloaded {
        meta_row = meta_row.push(
            container(
                text("已下载")
                    .size(10)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None)
                    .color(palette.ok),
            )
            .padding([1, 5])
            .style(panel_style(palette.ok_soft, None, 4.0)),
        );
    } else if !matched {
        meta_row = meta_row.push(
            container(
                text("已过滤")
                    .size(10)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
            )
            .padding([1, 5])
            .style(panel_style(palette.bg_card, Some(palette.hairline), 4.0)),
        );
    }

    if let Some(sz) = item.size_bytes {
        meta_row = meta_row.push(
            text(format_bytes(sz))
                .size(11)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::None)
                .color(palette.text_secondary),
        );
    }
    if let Some(ts) = &item.published_at {
        meta_row = meta_row.push(
            text(ts)
                .size(11)
                .font(FONT_MONO)
                .wrapping(text::Wrapping::None)
                .color(palette.text_muted),
        );
    }
    if show_feed_name {
        meta_row = meta_row.push(
            container(
                text(feed.display_name())
                    .size(11)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None)
                    .color(palette.text_muted),
            )
            .width(Length::Fill)
            .clip(true),
        );
    }

    let mut actions = row![
        button(
            row![
                icon(
                    IconKind::Downloading,
                    12.0,
                    if downloaded {
                        palette.text_secondary
                    } else {
                        palette.cta_text
                    }
                ),
                text(if downloaded { "重下" } else { "下载" })
                    .size(12)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([5, 10])
        .style(move |theme, status| {
            if downloaded {
                outline_btn_style(palette)(theme, status)
            } else {
                primary_btn_style(palette)(theme, status)
            }
        })
        .on_press(Message::RssDownloadItem(feed.id, item.guid.clone())),
    ]
    .spacing(5)
    .align_y(Alignment::Center);

    if let Some(page) = &item.page_url {
        actions = actions.push(
            button(icon(IconKind::ExternalLink, 12.0, palette.text_secondary))
                .padding([5, 7])
                .style(outline_btn_style(palette))
                .on_press(Message::OpenExternalUrl(page.clone())),
        );
    }

    container(
        row![
            column![
                text(&item.title)
                    .size(13)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::WordOrGlyph)
                    .color(if downloaded || !matched {
                        palette.text_secondary
                    } else {
                        palette.text_primary
                    }),
                meta_row,
            ]
            .spacing(5)
            .width(Length::Fill),
            actions,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([9, 12])
    .style(panel_style(palette.bg_input, Some(palette.hairline), 8.0))
    .into()
}
