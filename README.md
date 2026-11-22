<p align="center">
  <img src="browser-extension/icons/icon-128.png" width="88" height="88" alt="Siphon Logo" />
</p>

<h1 align="center">Siphon Downloader</h1>

<p align="center">
  <strong>纯 Rust 构建的 Windows 11 高性能多线程下载器</strong><br/>
  动态二分借片 · IDM 风格独立捕获与详情小窗 · 浏览器右键资源提交 · 规则引擎 · 可控数据目录
</p>

<p align="center">
  <a href="https://github.com/COMICFLY/siphon-downloader/releases"><img src="https://img.shields.io/badge/release-v0.2.0-38BDF8?style=flat-square" alt="Release" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011%20x64-0EA5E9?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/language-Rust%202024-F97316?style=flat-square" alt="Rust" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-22C55E?style=flat-square" alt="License" /></a>
</p>

---

## 界面预览

### 主界面（Graphite 深色主题 · MiSans 字体 · 系统原生文件图标）
![Siphon 主界面](docs/screenshots/main-window.png)

### IDM 风格独立下载详情弹窗（自定义数据主题色 · 60s 实时吞吐曲线 · 无缝分片进度条）
![Siphon 独立下载详情弹窗](docs/screenshots/download-popup.png)

---

## 主要功能

### 1. 高性能纯 Rust 多线程 HTTP/HTTPS 下载引擎
- **真多线程并发连接**：基于独立 HTTP/1.1 TCP 并发连接与 `TCP_NODELAY`，避免 HTTP/2 单连接流控瓶颈，充分跑满千兆宽带（实测单任务峰值突破 `130 MB/s`）。
- **动态二分借片（Dynamic Work Stealing）**：活跃分片完成后自动二分接管最慢分片的剩余区间，彻底消除长尾卡顿。
- **256 KB 写缓冲合并落盘**：减少高并发下的磁盘随机 I/O 系统调用，低 CPU / 内存占用。
- **可靠断点续传与防损坏保护**：SQLite 实时持久化分片偏移、恢复时校验 `ETag` 与文件大小、支持过期直链无缝替换续传，内置 **SHA-256 / MD5 / BLAKE3** 哈希完整性校验。
- **分级令牌桶限速**：同时支持全局限速与单任务动态调速、在线增减线程数（1–32 线程）。

### 2. IDM 风格独立捕获小窗 & 独立下载详情小窗
- **独立悬浮捕获小窗**：即使主窗口最小化常驻托盘，捕获到浏览器或剪贴板下载链接时也会独立置顶弹出，支持修改文件名、选择保存目录、查看断点续传/匹配规则/浏览器凭证状态，并提供 **开始下载 / 后台下载 / 稍后下载 / 取消** 四种动作。
- **独立下载详情监控窗**：点击「开始下载」后弹出独立任务监控窗，集中呈现传输状态、实时速率、剩余时间、60 秒吞吐曲线与连续无缝分片进度条；支持**转入后台**、**下载完成后自动关闭窗口**、完成后一键打开文件或定位所在目录。

### 3. Chrome / Edge 浏览器下载捕获扩展（MV3 独立分发）
- **自动拦截接管**：监听本地 `127.0.0.1:16800` 桥接，自动携带浏览器 `Cookie`、`Referer` 与 `User-Agent` 接管大文件与指定格式下载。
- **网页资源全场景右键提交**：支持在网页任意**超链接、图片、音视频、选中文本链接、页面空白处**右键直接提交至 Siphon（含捕获弹窗与后台静默直下两种模式）。
- **当前页资源嗅探**：点击浏览器工具栏扩展图标即可列出当前页探测到的直链、视频、音频及大图资源，一键发送到 Siphon。
- **灵活分发方式**：安装包与 Release 页面均提供**独立 `browser-extension/` 文件夹**与**独立 `.zip` 扩展压缩包**，软件内亦内置自动识别默认浏览器的引导式安装向导与 `.zip` 一键导出。

### 4. 自动化规则引擎 & 外部插件协议
- **可视化规则引擎**：支持按文件后缀、域名通配符、优先级配置自动归档目录（支持 `{base}`、`{domain}`、`{category}`、`{date}`、`{ext}` 变量与原生目录选择器）以及自动覆盖线程数。
- **跨语言外部插件系统**：通过 `plugin.toml` 与 JSON `stdin/stdout` 协议，可使用 Python / PowerShell / Go / Rust 等任意语言扩展站点解析（`resolve`）与下载后处理（`post_download`）钩子，支持一键导入外部插件目录。

### 5. 原生 Windows 11 体验与可控数据目录
- **视觉与交互**：深浅双 Graphite 主题、Windows 11 Mica 材质、**主界面主题色与独立下载弹窗数据色双路自定义**（8 款精选预设 + 任意 `#RRGGBB` HEX 色值）、MiSans OTF 抗锯齿字体、**实时随 Windows 默认关联变化的文件图标**、拖入链接与右键/Win+V 剪贴板粘贴、删除任务防误删弹窗确认。
- **完全可控的数据目录**：所有配置文件（`config.toml`）、数据库（`siphon.db`）与插件目录均支持用户自由指定与迁移：
  1. 命令行参数：`siphon.exe --data-dir "D:\SiphonData"`
  2. 环境变量：`SIPHON_DATA_DIR=D:\SiphonData`
  3. 安装程序自定义 / 便携模式：程序同目录下的 `data_dir.txt`（支持绝对路径或相对路径如 `.\data`）
  4. 软件内切换：**设置 → 软件数据目录 → 更改目录**（自动迁移已有配置与数据库）
  5. 默认目录：`%APPDATA%\Siphon`
- **开机静默自启 & 在线检查更新**：支持安装阶段与软件设置双向同步的 Windows 开机静默自启（`--minimized` 常驻托盘），以及内置 GitHub Releases 在线检查更新与一键拉起 Siphon 下载新版本安装包。

---

## 未来规划（Roadmap）

以下功能已在规划路线中，将在后续版本逐步落地：

- [ ] **BitTorrent / Magnet 协议引擎**：原生支持 `.torrent` 种子与磁力链接高速下载、选择性文件下载与 DHT/Tracker 调度。
- [ ] **流媒体嗅探与自动合并**：支持浏览器端 HLS（`.m3u8`）与 DASH（`.mpd`）音视频分片嗅探、解密与自动无损封装为 MP4 / MKV。
- [ ] **定时任务与队列调度器**：支持预约定时开始/暂停、分时段自动限速、下载队列完成后自动关机/睡眠或执行自定义脚本。
- [ ] **批量通配符队列导入**：支持 URL 序号通配符批量生成任务（如 `https://example.com/file_[001-100].zip`）与剪贴板多行链接批量过滤导入。
- [ ] **站点凭证与高级网络控制**：支持按域名独立配置 HTTP Basic / Cookie / 自定义 Header、分域名限速、以及 HTTP/SOCKS5 代理分流规则。
- [ ] **局域网 / WebUI 远程管理**：内置轻量 REST API 与 Web 控制台，支持手机或局域网其他设备远程推送下载任务并查看进度。
- [ ] **应用内静默增量更新**：检测到新版本后支持在软件内直接下载校验并自动完成覆盖升级。

---

## 下载与安装

前往 [**GitHub Releases**](https://github.com/COMICFLY/siphon-downloader/releases) 下载对应文件：

1. **`Siphon-Setup-0.2.0-x64.exe`（推荐）**：标准 Windows 安装包
   - 支持自由选择**程序安装目录**与**数据存储目录**；
   - 支持勾选创建桌面快捷方式与**开机静默启动**；
   - 安装目录内自带独立的 `browser-extension\` 文件夹与 `siphon-browser-extension-v0.2.0.zip` 扩展压缩包。
2. **`siphon-browser-extension-v0.2.0.zip`**：独立的 Chrome / Edge / Brave / Vivaldi 浏览器扩展压缩包
   - 解压后打开浏览器的 `chrome://extensions` 或 `edge://extensions`；
   - 开启右上角 **「开发者模式」**，将解压出的文件夹直接拖入浏览器扩展页即可完成加载。

---

## 从源码构建

需要安装 **Rust 1.85+**（Edition 2024）与 Windows 10/11 MSVC 工具链：

```powershell
# 克隆仓库
git clone https://github.com/COMICFLY/siphon-downloader.git
cd siphon-downloader

# 运行全部单元测试
cargo test --workspace

# 构建 Release 可执行文件
cargo build --release --workspace
```

编译产物位于 `target\release\siphon.exe` 与 `target\release\siphon-native-host.exe`。如需构建标准安装包，使用 [Inno Setup 6](https://jrsoftware.org/isinfo.php) 编译 `installer\siphon.iss` 即可。

---

## 作者与开源协议

- **作者**：[**COMICFLY**](https://github.com/COMICFLY)
- **项目主页**：[https://github.com/COMICFLY/siphon-downloader](https://github.com/COMICFLY/siphon-downloader)
- **开源协议**：本项目基于 [MIT License](LICENSE) 开源。
