use siphon_core::{FileCategory, Language, TaskStatus};

#[derive(Debug, Clone, Copy)]
pub struct Strings {
    pub lang: Language,
}

impl Strings {
    pub fn new(lang: Language) -> Self {
        Self { lang }
    }

    fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        match self.lang {
            Language::ZhCn => zh,
            Language::EnUs => en,
        }
    }

    pub fn quick_bar_placeholder(&self) -> &'static str {
        self.t("粘贴链接回车下载，或搜索", "Paste URL or search")
    }

    pub fn new_task(&self) -> &'static str {
        self.t("新建下载", "New task")
    }
    pub fn nav_views(&self) -> &'static str {
        self.t("任务", "Tasks")
    }
    pub fn nav_categories(&self) -> &'static str {
        self.t("分类", "Categories")
    }
    pub fn nav_workspace(&self) -> &'static str {
        self.t("工具", "Tools")
    }
    pub fn all_tasks(&self) -> &'static str {
        self.t("全部任务", "All downloads")
    }
    pub fn active_tasks(&self) -> &'static str {
        self.t("下载中", "Active")
    }
    pub fn completed_tasks(&self) -> &'static str {
        self.t("已完成", "Completed")
    }
    pub fn paused_tasks(&self) -> &'static str {
        self.t("队列与暂停", "Paused")
    }
    pub fn rules(&self) -> &'static str {
        self.t("规则引擎", "Rules")
    }
    pub fn plugins(&self) -> &'static str {
        self.t("插件扩展", "Plugins")
    }
    pub fn settings(&self) -> &'static str {
        self.t("设置", "Settings")
    }
    pub fn about(&self) -> &'static str {
        self.t("关于软件", "About")
    }
    pub fn pause_all(&self) -> &'static str {
        self.t("全部暂停", "Pause all")
    }
    pub fn resume_all(&self) -> &'static str {
        self.t("全部继续", "Resume all")
    }
    pub fn density_compact(&self) -> &'static str {
        self.t("紧凑", "Compact")
    }
    pub fn density_comfortable(&self) -> &'static str {
        self.t("舒适", "Cozy")
    }
    pub fn inspector_title(&self) -> &'static str {
        self.t("传输详情", "Inspector")
    }
    pub fn speed_chart_title(&self) -> &'static str {
        self.t("实时速率 · 60s", "Throughput · 60s")
    }
    pub fn segments_title(&self) -> &'static str {
        self.t("多线程分片", "Segments")
    }
    pub fn connections(&self) -> &'static str {
        self.t("并发线程", "Threads")
    }
    pub fn speed_limit(&self) -> &'static str {
        self.t("单任务限速", "Speed cap")
    }
    pub fn unlimited(&self) -> &'static str {
        self.t("不限速", "Unlimited")
    }
    pub fn open_folder(&self) -> &'static str {
        self.t("定位文件", "Reveal")
    }
    pub fn open_file(&self) -> &'static str {
        self.t("打开文件", "Open")
    }
    pub fn replace_link(&self) -> &'static str {
        self.t("替换直链续传", "Replace link")
    }
    pub fn delete_task(&self) -> &'static str {
        self.t("删除任务", "Delete")
    }
    pub fn logs_title(&self) -> &'static str {
        self.t("连接日志", "Connection log")
    }

    pub fn category_label(&self, cat: FileCategory) -> &'static str {
        match cat {
            FileCategory::Video => self.t("视频", "Video"),
            FileCategory::Archive => self.t("压缩包", "Archive"),
            FileCategory::Software => self.t("软件", "Software"),
            FileCategory::Document => self.t("文档", "Document"),
            FileCategory::DiskImage => self.t("镜像", "Disk image"),
            FileCategory::Audio => self.t("音频", "Audio"),
            FileCategory::Image => self.t("图片", "Picture"),
            FileCategory::Other => self.t("其他", "Other"),
        }
    }

    pub fn status_label(&self, status: TaskStatus) -> &'static str {
        match status {
            TaskStatus::Queued => self.t("排队中", "Queued"),
            TaskStatus::Probing => self.t("探测中", "Probing"),
            TaskStatus::Downloading => self.t("下载中", "Downloading"),
            TaskStatus::Paused => self.t("已暂停", "Paused"),
            TaskStatus::Verifying => self.t("校验中", "Verifying"),
            TaskStatus::Completed => self.t("已完成", "Completed"),
            TaskStatus::Error => self.t("失败", "Error"),
            TaskStatus::LinkExpired => self.t("链接过期", "Expired"),
        }
    }
}
