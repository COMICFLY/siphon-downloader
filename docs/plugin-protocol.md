# Siphon 插件与扩展开发指南

Siphon 的核心理念是**“由你自由扩展”**，提供两层扩展能力：

1. **零重编译外部脚本插件（推荐日常使用）**：使用任意语言（Python / PowerShell / Node.js / Rust / Go）通过 `plugin.toml` + `stdin/stdout JSON` 接入。
2. **编译期 Rust Trait 扩展**：在 Workspace 内实现 `ProtocolHandler`（新协议）、`LinkResolver`（内置直链解析）或 `PostProcessor`（下载后管线）。

---

## 1. 外部脚本插件规范

在以下任一目录中创建子文件夹并放置 `plugin.toml` 即可被 Siphon 自动扫描发现：

- 用户插件目录：`%APPDATA%\Siphon\plugins\<plugin-name>\plugin.toml`
- 工作区 / 同级目录：`./plugins/<plugin-name>/plugin.toml`

### `plugin.toml` 示例

```toml
name = "my-site-resolver"
version = "0.1.0"
description = "自动解析自定义站点直链并在下载后归档"
enabled = true
hooks = ["resolve", "post_download"]
url_patterns = ["*://example.com/*"]
command = ["python", "main.py"]
timeout_secs = 15
```

### `resolve` 钩子协议（下载前）

Siphon 向插件 `stdin` 写入单行 JSON：

```json
{
  "hook": "resolve",
  "url": "https://example.com/share/abc",
  "headers": {}
}
```

插件向 `stdout` 输出 JSON（可修改直链、注入 Cookie/Header、指定文件名或展开成批量子任务）：

```json
{
  "ok": true,
  "url": "https://cdn.example.com/real-file.zip?token=xyz",
  "headers": {
    "Referer": "https://example.com/",
    "Cookie": "session=..."
  },
  "filename": "real-file.zip",
  "extra_tasks": [],
  "note": "解析成功"
}
```

### `post_download` 钩子协议（完成后）

Siphon 向插件 `stdin` 写入：

```json
{
  "hook": "post_download",
  "task_id": "...",
  "url": "https://cdn.example.com/real-file.zip",
  "file_path": "C:\\Users\\You\\Downloads\\Archives\\real-file.zip",
  "filename": "real-file.zip",
  "total_bytes": 104857600,
  "verified_hash": null
}
```

插件向 `stdout` 输出：

```json
{
  "ok": true,
  "note": "已自动通知并归档"
}
```

---

## 2. 本地 IPC 协议（Windows Named Pipe）

Siphon 主程序监听 `\\.\pipe\siphon-ipc`，外部工具或浏览器桥接器（`siphon-native-host`）可发送换行分隔 JSON 指令：

```json
{"type":"show_window"}
{"type":"add_download","url":"https://example.com/a.iso","filename":null,"headers":{},"from_browser":true}
```
