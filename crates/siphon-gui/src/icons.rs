use iced::widget::svg;
use iced::{Color, Length};
use siphon_core::FileCategory;

#[derive(Debug, Clone, Copy)]
pub enum IconKind {
    Brand,
    AllTasks,
    Downloading,
    Completed,
    Paused,
    Video,
    Archive,
    Software,
    Document,
    DiskImage,
    Other,
    Rules,
    Plugins,
    Settings,
    Plus,
    Play,
    Pause,
    Folder,
    Edit,
    Trash,
    Link,
    Refresh,
    Sun,
    Moon,
    Close,
    Minimize,
    Maximize,
    Search,
    Bolt,
    Info,
    ExternalLink,
    Clock,
    Layers,
    Magnet,
    Server,
    Rss,
}

impl IconKind {
    pub fn from_category(cat: FileCategory) -> Self {
        match cat {
            FileCategory::Video => Self::Video,
            FileCategory::Archive => Self::Archive,
            FileCategory::Software => Self::Software,
            FileCategory::Document => Self::Document,
            FileCategory::DiskImage => Self::DiskImage,
            _ => Self::Other,
        }
    }

    fn svg_bytes(self) -> &'static [u8] {
        // Unified 24x24 Lucide-style geometry with consistent 1.65 stroke weight
        match self {
            Self::Brand => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v8"/><path d="M8.75 11.75L12 15l3.25-3.25"/><path d="M8.5 17.5h7"/></svg>"##,
            Self::AllTasks => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M8 9.5h8"/><path d="M8 13.5h5"/></svg>"##,
            Self::Downloading => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4.5v10.5"/><path d="M7.75 11.25L12 15.5l4.25-4.25"/><path d="M5 19.5h14"/></svg>"##,
            Self::Completed => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.5"/><path d="M8.75 12.25l2.2 2.2 4.4-4.7"/></svg>"##,
            Self::Paused => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.5"/><path d="M10 9v6"/><path d="M14 9v6"/></svg>"##,
            Self::Video => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="5.5" width="17" height="13" rx="2.5"/><polygon points="10.2 9.6 15.2 12 10.2 14.4 10.2 9.6"/></svg>"##,
            Self::Archive => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="4.5" width="17" height="4.5" rx="1.2"/><path d="M5.5 9v9a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V9"/><path d="M10 13.5h4"/></svg>"##,
            Self::Software => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3.5l7.5 4.25v8.5L12 20.5l-7.5-4.25v-8.5L12 3.5z"/><path d="M12 12l7.5-4.25"/><path d="M12 12v8.5"/><path d="M12 12L4.5 7.75"/></svg>"##,
            Self::Document => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5z"/><path d="M14 3.5v5h5"/><path d="M9 13h6"/><path d="M9 16.5h4"/></svg>"##,
            Self::DiskImage => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="2.2"/></svg>"##,
            Self::Other => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M13.5 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9z"/><path d="M13.5 3.5V9H19"/></svg>"##,
            Self::Rules => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7.5h10"/><circle cx="16.5" cy="7.5" r="2.2"/><path d="M20 16.5H10"/><circle cx="7.5" cy="16.5" r="2.2"/></svg>"##,
            Self::Plugins => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v4.5"/><path d="M8 6.5h8v3.5a4 4 0 0 1-8 0V6.5z"/><path d="M12 14v3.5"/><path d="M7.5 20h9"/></svg>"##,
            Self::Settings => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01A1.65 1.65 0 0 0 10 3.09V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>"##,
            Self::Plus => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14"/><path d="M5 12h14"/></svg>"##,
            Self::Play => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><polygon points="7.5 5.5 18.5 12 7.5 18.5 7.5 5.5"/></svg>"##,
            Self::Pause => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><rect x="6.5" y="5.5" width="3.8" height="13" rx="1"/><rect x="13.7" y="5.5" width="3.8" height="13" rx="1"/></svg>"##,
            Self::Folder => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 7a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/></svg>"##,
            Self::Edit => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5z"/></svg>"##,
            Self::Trash => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 7h15"/><path d="M10 11v5.5"/><path d="M14 11v5.5"/><path d="M6.5 7l1 11.5a2 2 0 0 0 2 1.8h5a2 2 0 0 0 2-1.8l1-11.5"/><path d="M9.5 7V4.5a1 1 0 0 1 1-1h3a1 1 0 0 1 1 1V7"/></svg>"##,
            Self::Link => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a4.8 4.8 0 0 0 6.8 0l2-2a4.8 4.8 0 0 0-6.8-6.8l-1.2 1.2"/><path d="M14 11a4.8 4.8 0 0 0-6.8 0l-2 2a4.8 4.8 0 0 0 6.8 6.8l1.2-1.2"/></svg>"##,
            Self::Refresh => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M20.5 12a8.5 8.5 0 1 1-2.5-6"/><path d="M20.5 4.5V9H16"/></svg>"##,
            Self::Sun => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round"><circle cx="12" cy="12" r="3.8"/><path d="M12 2.5v2"/><path d="M12 19.5v2"/><path d="M5.3 5.3l1.4 1.4"/><path d="M17.3 17.3l1.4 1.4"/><path d="M2.5 12h2"/><path d="M19.5 12h2"/><path d="M5.3 18.7l1.4-1.4"/><path d="M17.3 6.7l1.4-1.4"/></svg>"##,
            Self::Moon => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M20.5 13A8.5 8.5 0 1 1 11 3.5 6.8 6.8 0 0 0 20.5 13z"/></svg>"##,
            Self::Close => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6.5 6.5l11 11"/><path d="M17.5 6.5l-11 11"/></svg>"##,
            Self::Minimize => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M5 11.2H19V13.2H5Z" fill="#FFFFFF"/></svg>"##,
            Self::Maximize => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><rect x="5.5" y="5.5" width="13" height="13" rx="1.6"/></svg>"##,
            Self::Search => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.2-4.2"/></svg>"##,
            Self::Bolt => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2.5 4.5 14 12 14 11 21.5 19.5 10 12 10 13 2.5"/></svg>"##,
            Self::Info => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 16v-4"/><circle cx="12" cy="8.5" r="0.6" fill="currentColor"/></svg>"##,
            Self::ExternalLink => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 5h5v5"/><path d="M10 14L19 5"/><path d="M19 13v5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h5"/></svg>"##,
            Self::Clock => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/></svg>"##,
            Self::Layers => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 3.5 3.5 8 12 12.5 20.5 8 12 3.5"/><polyline points="3.5 12.5 12 17 20.5 12.5"/><polyline points="3.5 16.5 12 21 20.5 16.5"/></svg>"##,
            Self::Magnet => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M6 4.5v8a6 6 0 0 0 12 0v-8"/><path d="M6 8.5h4v4a2 2 0 0 0 4 0v-4h4"/><path d="M6 4.5h4v4H6z"/><path d="M14 4.5h4v4h-4z"/></svg>"##,
            Self::Server => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="4" width="17" height="6.5" rx="2"/><rect x="3.5" y="13.5" width="17" height="6.5" rx="2"/><circle cx="7.5" cy="7.25" r="0.8" fill="currentColor"/><circle cx="7.5" cy="16.75" r="0.8" fill="currentColor"/><path d="M11.5 7.25h5"/><path d="M11.5 16.75h5"/></svg>"##,
            Self::Rss => br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 11.5a8 8 0 0 1 8 8"/><path d="M4.5 4.5a15 15 0 0 1 15 15"/><circle cx="5.5" cy="18.5" r="1.4" fill="currentColor"/></svg>"##,
        }
    }
}

pub fn icon(kind: IconKind, size: f32, color: Color) -> svg::Svg<'static> {
    svg(svg::Handle::from_memory(kind.svg_bytes()))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(color) })
}
