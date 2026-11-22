use crate::icons::{IconKind, icon};
use crate::state::{InputTarget, Message, RuleDraft};
use crate::theme::{
    FONT_MEDIUM, FONT_MONO, FONT_REGULAR, FONT_SEMIBOLD, SiphonPalette, clean_scrollable_style,
    danger_btn_style, input_style, outline_btn_style, panel_style, primary_btn_style,
};
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, space, text, text_input, toggler,
};
use iced::{Alignment, Color, Element, Length};
use siphon_core::{AutomationRule, RuleCondition};

pub fn rule_name_input_id() -> iced::widget::Id {
    iced::widget::Id::new("siphon-rule-name")
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

pub fn view<'a>(
    rules: &'a [AutomationRule],
    draft: &'a RuleDraft,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    let is_editing = draft.editing_id.is_some();

    let header = column![
        text("规则引擎")
            .size(22)
            .font(FONT_SEMIBOLD)
            .color(palette.text_primary),
        text("按优先级匹配下载任务，支持 {base} / {date} / {domain} / {category} / {ext} 目录模板或直接选择文件夹")
            .size(14)
            .font(FONT_REGULAR)
            .color(palette.text_secondary),
    ]
    .spacing(4);

    let editing_has_extra_conditions = draft
        .editing_id
        .and_then(|id| rules.iter().find(|r| r.id == id))
        .is_some_and(|r| {
            r.conditions.iter().any(|c| {
                !matches!(
                    c,
                    RuleCondition::Extensions(_) | RuleCondition::DomainGlob(_)
                )
            })
        });

    let can_submit = !draft.name.trim().is_empty()
        && (!draft.extensions.trim().is_empty()
            || !draft.domain_glob.trim().is_empty()
            || editing_has_extra_conditions);

    let mut form_header = row![
        text(if is_editing { "编辑规则" } else { "新增规则" })
            .size(15)
            .font(FONT_MEDIUM)
            .color(palette.text_primary),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    if is_editing {
        form_header = form_header
            .push(
                container(
                    text("编辑中")
                        .size(12)
                        .font(FONT_MEDIUM)
                        .color(palette.accent),
                )
                .padding([2, 8])
                .style(panel_style(palette.accent_soft, None, 6.0)),
            )
            .push(space::horizontal())
            .push(
                button(
                    text("取消编辑")
                        .size(12)
                        .font(FONT_REGULAR)
                        .wrapping(text::Wrapping::None),
                )
                .padding([5, 12])
                .style(outline_btn_style(palette))
                .on_press(Message::RuleEditCancel),
            );
    }

    let save_dir_row = row![
        wrap_input(
            text_input(r"{base}\{domain}", &draft.save_dir_template)
                .on_input(Message::RuleDraftSaveDir)
                .font(FONT_MONO)
                .size(13)
                .padding([8, 10])
                .width(Length::Fill)
                .style(input_style(palette)),
            InputTarget::RuleSaveDir
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
        .padding([8, 12])
        .style(outline_btn_style(palette))
        .on_press(Message::RuleDraftBrowseDir),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let submit_btn = if is_editing {
        button(
            row![
                icon(IconKind::Edit, 14.0, palette.cta_text),
                text("保存修改")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([8, 16])
        .style(primary_btn_style(palette))
        .on_press_maybe(can_submit.then_some(Message::RuleAddSubmit))
    } else {
        button(
            row![
                icon(IconKind::Plus, 14.0, palette.cta_text),
                text("添加规则")
                    .size(13)
                    .font(FONT_MEDIUM)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([8, 16])
        .style(primary_btn_style(palette))
        .on_press_maybe(can_submit.then_some(Message::RuleAddSubmit))
    };

    let form_card = container(
        column![
            form_header,
            row![
                field_col(
                    "规则名称",
                    wrap_input(
                        text_input("例如：GitHub 安装包归档", &draft.name)
                            .id(rule_name_input_id())
                            .on_input(Message::RuleDraftName)
                            .font(FONT_REGULAR)
                            .size(13)
                            .padding([8, 10])
                            .style(input_style(palette)),
                        InputTarget::RuleName
                    ),
                    palette
                ),
                field_col(
                    "扩展名（逗号分隔）",
                    wrap_input(
                        text_input("zip,7z,exe", &draft.extensions)
                            .on_input(Message::RuleDraftExtensions)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([8, 10])
                            .style(input_style(palette)),
                        InputTarget::RuleExtensions
                    ),
                    palette
                ),
                field_col(
                    "域名通配",
                    wrap_input(
                        text_input("*.github.com", &draft.domain_glob)
                            .on_input(Message::RuleDraftDomain)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([8, 10])
                            .style(input_style(palette)),
                        InputTarget::RuleDomain
                    ),
                    palette
                ),
                container(field_col(
                    "优先级",
                    wrap_input(
                        text_input("自动", &draft.priority)
                            .on_input(Message::RuleDraftPriority)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([8, 10])
                            .style(input_style(palette)),
                        InputTarget::RulePriority
                    ),
                    palette
                ))
                .width(Length::Fixed(88.0)),
            ]
            .spacing(12),
            row![
                field_col("目标保存目录（模板或本地文件夹）", save_dir_row, palette),
                container(field_col(
                    "覆写线程数",
                    wrap_input(
                        text_input("默认", &draft.connections)
                            .on_input(Message::RuleDraftConnections)
                            .font(FONT_MONO)
                            .size(13)
                            .padding([8, 10])
                            .style(input_style(palette)),
                        InputTarget::RuleConnections
                    ),
                    palette
                ))
                .width(Length::Fixed(108.0)),
                container(submit_btn).align_y(Alignment::End),
            ]
            .spacing(12)
            .align_y(Alignment::End),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(panel_style(
        palette.bg_card,
        Some(if is_editing {
            palette.accent
        } else {
            palette.hairline
        }),
        12.0,
    ));

    let mut list_col = column![].spacing(8);
    for rule in rules {
        let row_editing = draft.editing_id == Some(rule.id);

        let cond_summary = rule
            .conditions
            .iter()
            .map(|c| match c {
                RuleCondition::Extensions(exts) => format!("后缀: {}", exts.join(", ")),
                RuleCondition::DomainGlob(g) => format!("域名: {g}"),
                RuleCondition::DomainRegex(r) => format!("正则: {r}"),
                RuleCondition::MimePrefix(m) => format!("MIME: {m}*"),
                RuleCondition::MinSizeBytes(b) => format!("大小 >= {} B", b),
                RuleCondition::MaxSizeBytes(b) => format!("大小 <= {} B", b),
                RuleCondition::Source(s) => format!("来源: {s:?}"),
            })
            .collect::<Vec<_>>()
            .join("  ·  ");

        let dir_tpl = rule
            .action
            .save_dir_template
            .as_deref()
            .unwrap_or("默认下载目录");
        let conn_str = rule
            .action
            .connections_override
            .map(|c| format!(" · {c} 线程"))
            .unwrap_or_default();

        let left_tick = container(space())
            .width(Length::Fixed(3.0))
            .height(Length::Fixed(36.0))
            .style(panel_style(
                if row_editing {
                    palette.accent
                } else {
                    Color::TRANSPARENT
                },
                None,
                2.0,
            ));

        let edit_btn = button(
            row![
                icon(
                    IconKind::Edit,
                    13.0,
                    if row_editing {
                        palette.accent
                    } else {
                        palette.text_secondary
                    }
                ),
                text(if row_editing { "编辑中" } else { "编辑" })
                    .size(12)
                    .font(FONT_REGULAR)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .style(outline_btn_style(palette))
        .on_press(if row_editing {
            Message::RuleEditCancel
        } else {
            Message::RuleEditStart(rule.id)
        });

        let card = container(
            row![
                left_tick,
                toggler(rule.enabled).on_toggle(move |_| Message::RuleToggleEnabled(rule.id)),
                column![
                    row![
                        text(&rule.name)
                            .size(14)
                            .font(FONT_MEDIUM)
                            .color(palette.text_primary),
                        text(format!("P{}", rule.priority))
                            .size(12)
                            .font(FONT_MONO)
                            .color(palette.text_muted),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                    text(cond_summary)
                        .size(13)
                        .font(FONT_REGULAR)
                        .color(palette.text_secondary),
                    text(format!("目标: {dir_tpl}{conn_str}"))
                        .size(12)
                        .font(FONT_MONO)
                        .wrapping(text::Wrapping::WordOrGlyph)
                        .color(palette.text_muted),
                ]
                .spacing(4)
                .width(Length::Fill),
                row![
                    edit_btn,
                    button(icon(IconKind::Trash, 14.0, palette.err))
                        .padding([6, 8])
                        .style(danger_btn_style(palette))
                        .on_press(Message::RuleDelete(rule.id)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding(iced::Padding {
            top: 14.0,
            right: 16.0,
            bottom: 14.0,
            left: 8.0,
        })
        .width(Length::Fill)
        .style(panel_style(
            if row_editing {
                palette.bg_raised
            } else {
                palette.bg_card
            },
            Some(if row_editing {
                palette.accent
            } else {
                palette.hairline
            }),
            10.0,
        ));

        list_col = list_col.push(card);
    }

    let body = column![header, form_card, list_col]
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

fn field_col<'a>(
    label: &'static str,
    input: impl Into<Element<'a, Message>>,
    palette: SiphonPalette,
) -> Element<'a, Message> {
    column![
        text(label)
            .size(12)
            .font(FONT_MEDIUM)
            .color(palette.text_secondary),
        input.into(),
    ]
    .spacing(5)
    .width(Length::Fill)
    .into()
}
