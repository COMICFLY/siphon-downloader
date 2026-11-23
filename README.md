<p align="center">
  <img src="browser-extension/icons/icon-128.png" width="84" height="84" alt="Siphon Logo" />
</p>

<h1 align="center">Siphon Downloader</h1>

<p align="center">
  <strong>纯 Rust 构建的 Windows 11 多协议多线程下载器</strong>
</p>

<p align="center">
  <a href="https://github.com/COMICFLY/siphon-downloader/releases"><img src="https://img.shields.io/badge/release-v0.8.0-38BDF8?style=flat-square" alt="Release" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011%20x64-0EA5E9?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/language-Rust%202024-F97316?style=flat-square" alt="Rust" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-22C55E?style=flat-square" alt="License" /></a>
</p>

---

## 界面预览

![Siphon 主界面](docs/screenshots/main-window.png)

![Siphon 独立下载详情弹窗](docs/screenshots/download-popup.png)

---

## 核心功能

- **多协议纯 Rust 引擎**：HTTP/HTTPS 真多线程动态借片（支持断点续传、过期直链替换与哈希校验）、HLS（`.m3u8` AES-128 解密合并）、BitTorrent / 磁力链接与 eD2k 电驴（MD4 分块校验、`server.met` 订阅）。
- **BitTorrent 深度增强**：纯 Rust **MSE/PE（DH + RC4）协议加密**、IPv4+IPv6 双栈、自定义/订阅 Tracker 一键同步、实时红黄绿灯端口与 TUN 诊断，以及专属磁力/种子 Swarm 右侧详情面板。
- **独立弹窗与资源勾选**：悬浮下载捕获窗、网页资源嗅探选择窗、种子子文件 / eD2k 批量勾选窗，以及带 60 秒吞吐曲线与无缝分片进度条的独立监控小窗。
- **Chrome / Edge 浏览器扩展**：自动携带浏览器 Cookie / Referer 接管下载、未启动时自动拉起客户端，并支持按需深度嗅探 `.m3u8` / 音视频与磁力资源。
- **调度、规则与扩展**：支持定时启停队列、分时段限速、通配符批量导入（`[001-100]`、`[a-z]`）、自动化分类规则与跨语言外部插件。
- **原生 Windows 11 体验**：深浅 Graphite 主题与 Mica 材质、自定义主色与数据目录、开机静默常驻托盘，以及应用内一键静默更新。

---

## 下载与安装

前往 [**GitHub Releases**](https://github.com/COMICFLY/siphon-downloader/releases) 下载：

- **`Siphon-Setup-0.8.0-x64.exe`**：Windows 安装包（内置浏览器扩展文件夹与 `.zip` 包）
- **`siphon-browser-extension-v0.8.0.zip`**：独立 Chrome / Edge 浏览器扩展压缩包

---

<p align="center">
  Made by <a href="https://github.com/COMICFLY">COMICFLY</a> · <a href="LICENSE">MIT License</a>
</p>
