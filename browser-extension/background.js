const SIPHON_ENDPOINT = "http://127.0.0.1:16800";
const NATIVE_HOST_NAME = "com.siphon.downloader";
const CLIENT_VERSION = "chrome-extension/0.9.0";

const CAPTURE_EXT_RE = /\.(zip|7z|rar|tar|gz|bz2|xz|tgz|zst|iso|img|vhd|vhdx|dmg|ipsw|exe|msi|msix|appx|deb|rpm|apk|mp4|mkv|webm|mov|avi|flv|f4v|ts|m4s|m4v|m3u8|mpd|mp3|flac|wav|aac|ogg|m4a|opus|pdf|epub|doc|docx|xls|xlsx|ppt|pptx|torrent|bin|pak|safetensors|gguf)(\?|$)/i;
const M3U8_URL_RE = /\.(m3u8|m3u)(\?|$|&)/i;
const MPD_URL_RE = /\.mpd(\?|$|&)/i;
const VIDEO_URL_RE = /\.(mp4|mkv|webm|mov|avi|flv|f4v|m4v|m4s)(\?|$|&)/i;
const AUDIO_URL_RE = /\.(mp3|flac|wav|aac|ogg|m4a|opus)(\?|$|&)/i;
const QUERY_VIDEO_HINT_RE = /(?:mime_type|mime|format|media_type|type)=(?:video|mp4|flv|webm|m3u8|hls|mpegurl)/i;
const QUERY_AUDIO_HINT_RE = /(?:mime_type|mime|format|media_type|type)=(?:audio|mp3|m4a|aac|flac|ogg)/i;
const DOWNLOADABLE_URL_RE = /(?:https?:\/\/[^\s"'<>()\[\]{}]+|magnet:\?[^\s"'<>()\[\]{}]+|ed2k:\/\/\|file\|[^\s"'<>()\[\]{}]+)/gi;

const handledIds = new Set();

// Hydrate sniffEnabled reliably even right after MV3 service worker wake-up
let sniffEnabled = false;
let sniffFlagPromise = null;

function ensureSniffFlagLoaded() {
  if (!sniffFlagPromise) {
    sniffFlagPromise = new Promise((resolve) => {
      chrome.storage.local.get({ sniffEnabled: false }, (res) => {
        sniffEnabled = Boolean(res.sniffEnabled);
        resolve(sniffEnabled);
      });
    });
  }
  return sniffFlagPromise;
}

ensureSniffFlagLoaded();

// Write-through per-tab sniff cache backed by chrome.storage.session so MV3 SW sleep NEVER wipes sniffed streams!
const tabStreamsMem = new Map();
const tabAutoPoppedUrl = new Map();
const tabAutoTimers = new Map();

function sessionKey(tabId) {
  return `sniff_tab_${tabId}`;
}

async function getTabStreams(tabId) {
  if (tabStreamsMem.has(tabId)) {
    return tabStreamsMem.get(tabId);
  }
  try {
    const key = sessionKey(tabId);
    const res = await chrome.storage.session.get(key);
    const list = Array.isArray(res[key]) ? res[key] : [];
    tabStreamsMem.set(tabId, list);
    return list;
  } catch (_) {
    return [];
  }
}

async function saveTabStreams(tabId, list) {
  tabStreamsMem.set(tabId, list);
  try {
    await chrome.storage.session.set({ [sessionKey(tabId)]: list });
  } catch (_) {}
}

async function clearTabStreams(tabId) {
  tabStreamsMem.delete(tabId);
  tabAutoPoppedUrl.delete(tabId);
  if (tabAutoTimers.has(tabId)) {
    clearTimeout(tabAutoTimers.get(tabId));
    tabAutoTimers.delete(tabId);
  }
  try {
    await chrome.storage.session.remove(sessionKey(tabId));
  } catch (_) {}
}

async function clearAllSniffCaches() {
  tabStreamsMem.clear();
  tabAutoPoppedUrl.clear();
  for (const timer of tabAutoTimers.values()) {
    clearTimeout(timer);
  }
  tabAutoTimers.clear();
  try {
    await chrome.storage.session.clear();
  } catch (_) {}
  try {
    chrome.action.setBadgeText({ text: "" });
  } catch (_) {}
}

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes.sniffEnabled) {
    sniffEnabled = Boolean(changes.sniffEnabled.newValue);
    sniffFlagPromise = Promise.resolve(sniffEnabled);
    if (!sniffEnabled) {
      clearAllSniffCaches();
    }
  }
});

function isHttpUrl(url) {
  return typeof url === "string" && (url.startsWith("http://") || url.startsWith("https://"));
}

function isDownloadableUrl(url) {
  if (typeof url !== "string") return false;
  const low = url.toLowerCase();
  return (
    low.startsWith("http://") ||
    low.startsWith("https://") ||
    low.startsWith("magnet:?") ||
    low.startsWith("ed2k://|file|")
  );
}

function bridgeHeaders(extra = {}) {
  const h = {
    "X-Siphon-Client": CLIENT_VERSION,
    ...extra
  };
  try {
    if (chrome.runtime && chrome.runtime.id) {
      h["X-Siphon-Extension-Id"] = chrome.runtime.id;
    }
  } catch (_) {}
  return h;
}

function sendNativeToSiphon(ipcCommand) {
  return new Promise((resolve) => {
    try {
      chrome.runtime.sendNativeMessage(NATIVE_HOST_NAME, ipcCommand, (reply) => {
        if (chrome.runtime.lastError) {
          resolve({ ok: false, launched: false, error: chrome.runtime.lastError.message || "native_error" });
          return;
        }
        resolve(reply || { ok: false, launched: false });
      });
    } catch (e) {
      resolve({ ok: false, launched: false, error: String(e) });
    }
  });
}

function parseContentDispositionFilename(val) {
  if (!val) return null;
  const utf8Match = val.match(/filename\*\s*=\s*(?:UTF-8''|utf-8'')([^;\r\n]+)/i);
  if (utf8Match && utf8Match[1]) {
    try {
      return decodeURIComponent(utf8Match[1].trim().replace(/^["']|["']$/g, ""));
    } catch (_) {}
  }
  const plainMatch = val.match(/filename\s*=\s*"?([^";\r\n]+)"?/i);
  if (plainMatch && plainMatch[1]) {
    return plainMatch[1].trim();
  }
  return null;
}

async function recordNetworkStream(tabId, item) {
  if (!(await ensureSniffFlagLoaded())) return false;
  if (typeof tabId !== "number" || tabId < 0 || !isHttpUrl(item.url)) return false;
  if (item.url.startsWith("http://127.0.0.1") || item.url.startsWith("http://localhost")) return false;

  const list = (await getTabStreams(tabId)).slice();
  const existing = list.find((x) => x.url === item.url);
  if (existing) {
    let changed = false;
    if (!existing.file_size && item.file_size) {
      existing.file_size = item.file_size;
      changed = true;
    }
    if (!existing.mime && item.mime) {
      existing.mime = item.mime;
      changed = true;
    }
    if (!existing.filename && item.filename) {
      existing.filename = item.filename;
      changed = true;
    }
    if (changed) await saveTabStreams(tabId, list);
    return false;
  }

  list.unshift({
    url: item.url,
    filename: item.filename || null,
    kind: item.kind || "video",
    label: item.label || null,
    mime: item.mime || null,
    file_size: typeof item.file_size === "number" && item.file_size > 0 ? item.file_size : null
  });
  if (list.length > 80) {
    list.length = 80;
  }
  await saveTabStreams(tabId, list);

  // Update per-tab badge immediately so user sees live sniff count like BitComet!
  try {
    chrome.action.setBadgeBackgroundColor({ tabId, color: "#22C55E" });
    chrome.action.setBadgeText({ tabId, text: String(list.length) });
  } catch (_) {}

  return true;
}

function scheduleAutoSniffPush(tabId) {
  if (tabAutoTimers.has(tabId)) {
    clearTimeout(tabAutoTimers.get(tabId));
  }
  const timer = setTimeout(async () => {
    tabAutoTimers.delete(tabId);
    if (!(await ensureSniffFlagLoaded())) return;

    let tab = null;
    try {
      tab = await chrome.tabs.get(tabId);
    } catch (_) {
      return;
    }
    if (!tab || !tab.active || !tab.url || !isHttpUrl(tab.url)) return;

    const streams = (await getTabStreams(tabId)).slice();
    if (streams.length === 0) return;

    // Bring client selective sniff window to front once per page URL; silently merge subsequent streams
    const alreadyPopped = tabAutoPoppedUrl.get(tabId) === tab.url;
    tabAutoPoppedUrl.set(tabId, tab.url);

    await sendSniffToSiphon(tab, streams, !alreadyPopped);
  }, 600);
  tabAutoTimers.set(tabId, timer);
}

// 1. Sniff playlists & direct media URLs on request (ONLY when sniffEnabled is ON)
chrome.webRequest.onBeforeRequest.addListener(
  (details) => {
    if (details.tabId < 0) return;
    const url = details.url || "";
    let item = null;
    if (M3U8_URL_RE.test(url)) {
      item = { url, kind: "hls", label: "HLS Stream (.m3u8)", mime: "application/vnd.apple.mpegurl" };
    } else if (MPD_URL_RE.test(url)) {
      item = { url, kind: "hls", label: "DASH Stream (.mpd)", mime: "application/dash+xml" };
    } else if (QUERY_VIDEO_HINT_RE.test(url)) {
      item = { url, kind: "video", label: "视频流媒体" };
    } else if (QUERY_AUDIO_HINT_RE.test(url)) {
      item = { url, kind: "audio", label: "音频流媒体" };
    }
    if (!item) return;

    recordNetworkStream(details.tabId, item).then((added) => {
      if (added) scheduleAutoSniffPush(details.tabId);
    });
  },
  { urls: ["<all_urls>"] }
);

// 2. Sniff HLS / DASH / video / audio / octet-stream media / attachment via response headers
chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || !details.responseHeaders) return;
    let contentType = "";
    let contentLength = null;
    let contentRangeTotal = null;
    let contentDisposition = "";
    for (const h of details.responseHeaders) {
      if (!h.name || !h.value) continue;
      const name = h.name.toLowerCase();
      if (name === "content-type") {
        contentType = h.value.toLowerCase().split(";")[0].trim();
      } else if (name === "content-length") {
        const parsed = parseInt(h.value, 10);
        if (!Number.isNaN(parsed) && parsed > 0) contentLength = parsed;
      } else if (name === "content-range") {
        const m = h.value.match(/\/(\d+)\s*$/);
        if (m && m[1]) {
          const total = parseInt(m[1], 10);
          if (!Number.isNaN(total) && total > 0) contentRangeTotal = total;
        }
      } else if (name === "content-disposition") {
        contentDisposition = h.value;
      }
    }
    const effectiveSize = contentRangeTotal !== null ? contentRangeTotal : contentLength;

    const url = details.url || "";
    const cdName = parseContentDispositionFilename(contentDisposition);
    let kind = null;
    let label = null;

    if (
      contentType.includes("application/vnd.apple.mpegurl") ||
      contentType.includes("application/x-mpegurl") ||
      contentType.includes("audio/mpegurl") ||
      M3U8_URL_RE.test(url)
    ) {
      kind = "hls";
      label = "HLS Stream (.m3u8)";
    } else if (contentType.includes("application/dash+xml") || MPD_URL_RE.test(url)) {
      kind = "hls";
      label = "DASH Stream (.mpd)";
    } else if (
      (contentType.startsWith("video/") && !contentType.includes("mp2t")) ||
      VIDEO_URL_RE.test(url) ||
      QUERY_VIDEO_HINT_RE.test(url)
    ) {
      if (effectiveSize === null || effectiveSize >= 128 * 1024) {
        kind = "video";
        label = contentType ? `视频 (${contentType.replace("video/", "")})` : "视频流";
      }
    } else if (
      contentType.startsWith("audio/") ||
      AUDIO_URL_RE.test(url) ||
      QUERY_AUDIO_HINT_RE.test(url)
    ) {
      if (effectiveSize === null || effectiveSize >= 48 * 1024) {
        kind = "audio";
        label = contentType ? `音频 (${contentType.replace("audio/", "")})` : "音频流";
      }
    } else if (
      (contentType === "application/octet-stream" || contentType === "binary/octet-stream") &&
      effectiveSize !== null &&
      effectiveSize >= 512 * 1024
    ) {
      kind = VIDEO_URL_RE.test(url) ? "video" : "file";
      label = cdName || "大文件二进制流";
    } else if (contentDisposition.toLowerCase().includes("attachment") && cdName) {
      kind = "file";
      label = cdName;
    }

    if (!kind) return;

    recordNetworkStream(details.tabId, {
      url,
      filename: cdName,
      kind,
      label,
      mime: contentType || null,
      file_size: effectiveSize
    }).then((added) => {
      if (added) scheduleAutoSniffPush(details.tabId);
    });
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

chrome.tabs.onRemoved.addListener((tabId) => {
  clearTabStreams(tabId);
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === "loading" && changeInfo.url) {
    clearTabStreams(tabId);
  }
});

async function notifySiphonAlive() {
  try {
    await fetch(`${SIPHON_ENDPOINT}/api/ping`, {
      method: "GET",
      headers: bridgeHeaders()
    });
  } catch (_) {}
}

let badgeTimer = null;
function flashBadge(text, color) {
  try {
    if (badgeTimer) clearTimeout(badgeTimer);
    chrome.action.setBadgeBackgroundColor({ color });
    chrome.action.setBadgeText({ text });
    badgeTimer = setTimeout(() => {
      chrome.action.setBadgeText({ text: "" });
    }, 2600);
  } catch (_) {}
}

function registerContextMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "siphon-download-link",
      title: "使用 Siphon 下载此链接",
      contexts: ["link"]
    });
    chrome.contextMenus.create({
      id: "siphon-download-link-silent",
      title: "使用 Siphon 后台静默下载链接",
      contexts: ["link"]
    });
    chrome.contextMenus.create({
      id: "siphon-download-image",
      title: "使用 Siphon 下载此图片",
      contexts: ["image"]
    });
    chrome.contextMenus.create({
      id: "siphon-download-image-silent",
      title: "使用 Siphon 后台静默下载图片",
      contexts: ["image"]
    });
    chrome.contextMenus.create({
      id: "siphon-download-media",
      title: "使用 Siphon 下载此音视频",
      contexts: ["video", "audio"]
    });
    chrome.contextMenus.create({
      id: "siphon-download-media-silent",
      title: "使用 Siphon 后台静默下载音视频",
      contexts: ["video", "audio"]
    });
    chrome.contextMenus.create({
      id: "siphon-download-selection",
      title: "使用 Siphon 下载选中文本中的链接",
      contexts: ["selection"]
    });
    chrome.contextMenus.create({
      id: "siphon-sniff-page",
      title: "使用 Siphon 嗅探当前页资源（选择性下载窗）",
      contexts: ["page", "frame", "video", "audio"]
    });
  });
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({ enabled: true, sniffEnabled: false });
  sniffEnabled = false;
  sniffFlagPromise = Promise.resolve(false);
  clearAllSniffCaches();
  registerContextMenus();
  notifySiphonAlive();
});

chrome.runtime.onStartup.addListener(() => {
  sniffFlagPromise = null;
  ensureSniffFlagLoaded();
  registerContextMenus();
  notifySiphonAlive();
});

notifySiphonAlive();

function extractUrlsFromText(text) {
  if (!text) return [];
  const matches = text.match(DOWNLOADABLE_URL_RE) || [];
  const seen = new Set();
  const out = [];
  for (let raw of matches) {
    raw = raw.replace(/[.,;:!?)]+$/, "");
    if (isDownloadableUrl(raw) && !seen.has(raw)) {
      seen.add(raw);
      out.push(raw);
    }
  }
  return out;
}

/**
 * Deep multi-frame sniffer (main frame + ALL iframes):
 * 1) Live webRequest network streams (persisted in chrome.storage.session)
 * 2) `performance.getEntriesByType('resource')` — catches streams ALREADY loaded before enabling sniff!
 * 3) DOM `<video>`, `<audio>`, `<source>`, `<embed>`, `<object>`, `<a href>`, large `<img>`
 * 4) Inline script/JSON scan for hidden `.m3u8`, `.mpd`, `.mp4`, `.flv`, `magnet:?`, `ed2k://` URLs
 */
async function scanTabResources(tabId, mode) {
  if (!tabId) return [];
  const netSniffed = (await getTabStreams(tabId)).slice();
  let frameItems = [];

  try {
    const results = await chrome.scripting.executeScript({
      target: { tabId, allFrames: true },
      func: (scanMode) => {
        const extRe = /\.(zip|7z|rar|tar|gz|bz2|xz|tgz|zst|iso|img|vhd|vhdx|dmg|ipsw|exe|msi|msix|appx|deb|rpm|apk|mp4|mkv|webm|mov|avi|flv|f4v|ts|m4s|m4v|m3u8|mpd|mp3|flac|wav|aac|ogg|m4a|opus|pdf|epub|doc|docx|xls|xlsx|ppt|pptx|torrent|bin|pak|safetensors|gguf)(\?|$|&)/i;
        const m3u8Re = /\.(m3u8|m3u)(\?|$|&)/i;
        const mpdRe = /\.mpd(\?|$|&)/i;
        const videoRe = /\.(mp4|mkv|webm|mov|avi|flv|f4v|m4v|m4s)(\?|$|&)/i;
        const audioRe = /\.(mp3|flac|wav|aac|ogg|m4a|opus)(\?|$|&)/i;
        const queryVideoRe = /(?:mime_type|mime|format|media_type|type)=(?:video|mp4|flv|webm|m3u8|hls|mpegurl)/i;
        const queryAudioRe = /(?:mime_type|mime|format|media_type|type)=(?:audio|mp3|m4a|aac|flac|ogg)/i;

        const found = [];
        const seen = new Set();
        const resolveUrl = (raw) => {
          if (!raw || typeof raw !== "string") return "";
          const t = raw.trim();
          const low = t.toLowerCase();
          if (low.startsWith("magnet:?") || low.startsWith("ed2k://|file|")) return t;
          if (low.startsWith("blob:") || low.startsWith("data:") || low.startsWith("javascript:")) return "";
          try {
            return new URL(t, location.href).href;
          } catch (_) {
            return "";
          }
        };

        const add = (rawUrl, kind, label, fileSize = null) => {
          const url = resolveUrl(rawUrl);
          if (!url) return;
          const low = url.toLowerCase();
          if (
            !low.startsWith("http://") &&
            !low.startsWith("https://") &&
            !low.startsWith("magnet:?") &&
            !low.startsWith("ed2k://|file|")
          ) {
            return;
          }
          if (seen.has(url)) return;
          seen.add(url);
          found.push({
            url,
            filename: null,
            kind,
            label: (label || "").trim().slice(0, 80) || null,
            mime: null,
            file_size: typeof fileSize === "number" && fileSize > 0 ? fileSize : null
          });
        };

        // 1. Performance Resource Timing API — recovers streams already loaded before sniffer was opened!
        try {
          const entries = performance.getEntriesByType("resource") || [];
          for (const e of entries) {
            const u = e.name || "";
            if (!u || (!u.startsWith("http://") && !u.startsWith("https://"))) continue;
            const sz = (e.encodedBodySize || e.transferSize || 0) > 0 ? (e.encodedBodySize || e.transferSize) : null;
            if (m3u8Re.test(u)) {
              add(u, "hls", "HLS Stream (.m3u8)", sz);
            } else if (mpdRe.test(u)) {
              add(u, "hls", "DASH Stream (.mpd)", sz);
            } else if (videoRe.test(u) || queryVideoRe.test(u)) {
              if (sz === null || sz >= 64 * 1024) {
                add(u, "video", "已加载视频流", sz);
              }
            } else if (audioRe.test(u) || queryAudioRe.test(u)) {
              if (sz === null || sz >= 32 * 1024) {
                add(u, "audio", "已加载音频流", sz);
              }
            } else if (e.initiatorType === "video" || e.initiatorType === "audio") {
              add(u, e.initiatorType, "媒体请求", sz);
            }
          }
        } catch (_) {}

        // 2. DOM <video>, <audio>, <source>, <embed>, <object>
        document.querySelectorAll("video, audio").forEach((el) => {
          const kind = el.tagName.toLowerCase();
          const title = el.getAttribute("title") || el.getAttribute("aria-label") || kind;
          if (el.currentSrc) add(el.currentSrc, kind, title);
          if (el.src) add(el.src, kind, title);
          if (el.dataset && el.dataset.src) add(el.dataset.src, kind, title);
          el.querySelectorAll("source").forEach((src) => {
            const u = src.src || src.getAttribute("src") || (src.dataset && src.dataset.src);
            if (u) {
              const k = m3u8Re.test(u) || mpdRe.test(u) ? "hls" : kind;
              add(u, k, src.type || title);
            }
          });
        });

        document.querySelectorAll("embed[src], object[data]").forEach((el) => {
          const u = el.getAttribute("src") || el.getAttribute("data");
          if (u && (extRe.test(u) || m3u8Re.test(u))) {
            add(u, m3u8Re.test(u) ? "hls" : "video", "嵌入媒体");
          }
        });

        // 3. Inline script/JSON player config scan (catches escaped https:\/\/...m3u8 / mp4 / magnet / ed2k!)
        try {
          let blobText = "";
          const scripts = document.querySelectorAll("script");
          for (const s of scripts) {
            if (!s.src && s.textContent) {
              blobText += s.textContent + "\n";
              if (blobText.length > 400000) break;
            }
          }
          if (blobText.length > 0) {
            const unescaped = blobText.replace(/\\\//g, "/").replace(/\\u002F/gi, "/").replace(/\\u0026/gi, "&");
            const mediaMatches = unescaped.match(/https?:\/\/[^\s"'<>\\]+?\.(?:m3u8|mpd|mp4|flv|webm|mkv|mp3|m4a|flac)(?:\?[^\s"'<>\\]*)?/gi) || [];
            for (const raw of mediaMatches.slice(0, 40)) {
              const k = m3u8Re.test(raw) || mpdRe.test(raw)
                ? "hls"
                : audioRe.test(raw)
                  ? "audio"
                  : "video";
              add(raw, k, k === "hls" ? "页面脚本 HLS 流" : "页面脚本媒体");
            }
            const p2pMatches = unescaped.match(/(?:magnet:\?[^\s"'<>\\]+|ed2k:\/\/\|file\|[^\s"'<>\\]+)/gi) || [];
            for (const raw of p2pMatches.slice(0, 30)) {
              add(raw, raw.toLowerCase().startsWith("magnet:?") ? "magnet" : "ed2k", "页面内嵌资源");
            }
          }
        } catch (_) {}

        if (scanMode === "media") {
          return found;
        }

        // 4. Downloadable <a href> links, magnets, ed2k, torrents
        document.querySelectorAll("a[href]").forEach((a) => {
          const href = a.href || a.getAttribute("href");
          if (!href) return;
          const low = href.toLowerCase();
          if (low.startsWith("magnet:?")) {
            add(href, "magnet", a.textContent || "Magnet 磁力");
          } else if (low.startsWith("ed2k://|file|")) {
            add(href, "ed2k", a.textContent || "eD2k 电驴");
          } else if (a.hasAttribute("download") || extRe.test(href)) {
            const kind = m3u8Re.test(href) || mpdRe.test(href)
              ? "hls"
              : /\.torrent(\?|$)/i.test(href)
                ? "torrent"
                : videoRe.test(href)
                  ? "video"
                  : audioRe.test(href)
                    ? "audio"
                    : "file";
            add(href, kind, a.getAttribute("download") || a.textContent || "");
          }
        });

        if (scanMode === "all") {
          document.querySelectorAll("img[src]").forEach((img) => {
            if ((img.naturalWidth || 0) >= 200 && (img.naturalHeight || 0) >= 200) {
              add(img.currentSrc || img.src, "image", img.alt || "");
            }
          });
        }

        return found.slice(0, 100);
      },
      args: [mode]
    });

    if (Array.isArray(results)) {
      for (const frameRes of results) {
        if (frameRes && Array.isArray(frameRes.result)) {
          frameItems.push(...frameRes.result);
        }
      }
    }
  } catch (_) {}

  // Priority order: HLS -> Video -> Audio -> Magnet/Torrent/eD2k -> File -> Image
  const priority = (k) => {
    switch ((k || "").toLowerCase()) {
      case "hls": return 0;
      case "video": return 1;
      case "audio": return 2;
      case "magnet":
      case "torrent":
      case "ed2k": return 3;
      case "file": return 4;
      default: return 5;
    }
  };

  const merged = [];
  const seen = new Set();
  for (const item of [...netSniffed, ...frameItems]) {
    if (!item || !item.url) continue;
    if (!seen.has(item.url)) {
      seen.add(item.url);
      merged.push(item);
    }
  }
  merged.sort((a, b) => priority(a.kind) - priority(b.kind));
  return merged.slice(0, 160);
}

// Handle messages from popup.js
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (!msg || !msg.type) return;

  if (msg.type === "SCAN_ACTIVE_TAB") {
    chrome.tabs.query({ active: true, currentWindow: true }, async (tabs) => {
      const tab = tabs && tabs[0];
      if (!tab) {
        sendResponse({ ok: false, items: [] });
        return;
      }
      const items = await scanTabResources(tab.id, "all");
      sendResponse({
        ok: true,
        pageTitle: tab.title || "",
        referrer: tab.url || "",
        items
      });
    });
    return true;
  }

  if (msg.type === "OPEN_SNIFF_WINDOW") {
    chrome.tabs.query({ active: true, currentWindow: true }, async (tabs) => {
      const tab = tabs && tabs[0];
      if (!tab) {
        sendResponse({ ok: false, count: 0 });
        return;
      }
      const items = await scanTabResources(tab.id, "all");
      const ok = await sendSniffToSiphon(tab, items, true);
      flashBadge(ok ? String(items.length || "OK") : "!", ok ? "#22C55E" : "#E55353");
      sendResponse({ ok, count: items.length });
    });
    return true;
  }

  if (msg.type === "LAUNCH_SIPHON") {
    sendNativeToSiphon({ type: "launch" }).then((reply) => {
      const ok = Boolean(reply && reply.ok);
      flashBadge(ok ? "GO" : "!", ok ? "#22C55E" : "#E55353");
      sendResponse({ ok, reply });
    });
    return true;
  }

  if (msg.type === "SUBMIT_TO_SIPHON") {
    sendToSiphon({
      url: msg.url,
      filename: msg.filename || null,
      referrer: msg.referrer || "",
      mime: msg.mime || null,
      fileSize: null,
      silent: Boolean(msg.silent)
    }).then((ok) => {
      flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
      sendResponse({ ok });
    });
    return true;
  }
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const referrer = tab?.url || info.pageUrl || "";
  const id = info.menuItemId;

  if (id === "siphon-download-link" || id === "siphon-download-link-silent") {
    const targetUrl = info.linkUrl;
    if (!isDownloadableUrl(targetUrl)) return;
    const ok = await sendToSiphon({
      url: targetUrl,
      referrer,
      filename: null,
      mime: null,
      fileSize: null,
      silent: id === "siphon-download-link-silent"
    });
    flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
    return;
  }

  if (id === "siphon-download-image" || id === "siphon-download-image-silent") {
    const targetUrl = info.srcUrl || info.linkUrl;
    if (!isHttpUrl(targetUrl)) return;
    const ok = await sendToSiphon({
      url: targetUrl,
      referrer,
      filename: null,
      mime: "image/*",
      fileSize: null,
      silent: id === "siphon-download-image-silent"
    });
    flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
    return;
  }

  if (id === "siphon-download-media" || id === "siphon-download-media-silent") {
    const silent = id === "siphon-download-media-silent";
    let targetUrl = info.srcUrl;
    if (!isHttpUrl(targetUrl) && tab?.id) {
      const media = await scanTabResources(tab.id, "media");
      if (media.length > 0) {
        targetUrl = media[0].url;
      }
    }
    if (!isHttpUrl(targetUrl)) {
      flashBadge("0", "#E5A93B");
      return;
    }
    const ok = await sendToSiphon({
      url: targetUrl,
      referrer,
      filename: null,
      mime: info.mediaType === "audio" ? "audio/*" : "video/*",
      fileSize: null,
      silent
    });
    flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
    return;
  }

  if (id === "siphon-download-selection") {
    const urls = extractUrlsFromText(info.selectionText || "");
    if (urls.length === 0) {
      flashBadge("0", "#E5A93B");
      return;
    }
    if (urls.length === 1) {
      const ok = await sendToSiphon({
        url: urls[0],
        referrer,
        filename: null,
        mime: null,
        fileSize: null,
        silent: false
      });
      flashBadge(ok ? "OK" : "!", ok ? "#22C55E" : "#E55353");
    } else {
      const items = urls.map((u) => ({
        url: u,
        filename: null,
        kind: u.toLowerCase().startsWith("magnet:?")
          ? "magnet"
          : u.toLowerCase().startsWith("ed2k://")
            ? "ed2k"
            : M3U8_URL_RE.test(u) || MPD_URL_RE.test(u)
              ? "hls"
              : "file",
        label: null,
        mime: null,
        file_size: null
      }));
      const ok = await sendSniffToSiphon(tab || { title: "选中链接", url: referrer }, items, true);
      flashBadge(ok ? String(items.length) : "!", ok ? "#22C55E" : "#E55353");
    }
    return;
  }

  if (id === "siphon-sniff-page" && tab?.id) {
    const resources = await scanTabResources(tab.id, "all");
    const ok = await sendSniffToSiphon(tab, resources, true);
    flashBadge(ok ? String(resources.length || "OK") : "!", ok ? "#22C55E" : "#E55353");
  }
});

const IGNORE_EXT_RE = /\.(html?|mhtml|xhtml|css|js|json|xml)(\?|$)/i;

function shouldCaptureDownload(item, fromDeterminingFilename = false) {
  if (!item || item.byExtensionId) return false;
  const url = item.finalUrl || item.url || "";
  if (!isHttpUrl(url)) {
    return false;
  }
  if (url.startsWith("http://127.0.0.1") || url.startsWith("http://localhost")) {
    return false;
  }
  const fname = basename(item.filename) || "";
  if (fname && IGNORE_EXT_RE.test(fname)) {
    return false;
  }
  if (CAPTURE_EXT_RE.test(url) || (fname && CAPTURE_EXT_RE.test(fname))) {
    return true;
  }
  const mime = (item.mime || "").toLowerCase();
  if (
    mime.startsWith("application/octet-stream") ||
    mime.startsWith("application/x-") ||
    mime.startsWith("application/vnd.") ||
    mime.startsWith("application/zip") ||
    mime.startsWith("application/gzip") ||
    mime.startsWith("application/pdf") ||
    mime.startsWith("binary/") ||
    mime.startsWith("video/") ||
    mime.startsWith("audio/")
  ) {
    return true;
  }
  if (typeof item.fileSize === "number" && item.fileSize >= 64 * 1024) {
    return true;
  }
  if (fromDeterminingFilename && fname) {
    return true;
  }
  return false;
}

async function getCookieHeader(url) {
  if (!isHttpUrl(url)) return "";
  try {
    const cookies = await chrome.cookies.getAll({ url });
    if (!cookies || cookies.length === 0) return "";
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch (_) {
    return "";
  }
}

function basename(pathStr) {
  if (!pathStr) return null;
  const normalized = pathStr.replace(/\\/g, "/");
  const last = normalized.split("/").pop();
  return last || null;
}

async function sendToSiphon({ url, filename, referrer, mime, fileSize, silent = false }) {
  const cookie = await getCookieHeader(url);
  const payload = {
    url,
    filename: basename(filename),
    referrer: referrer || null,
    cookie: cookie || null,
    user_agent: navigator.userAgent,
    mime: mime || null,
    file_size: typeof fileSize === "number" && fileSize > 0 ? fileSize : null,
    silent: Boolean(silent)
  };

  try {
    const resp = await fetch(`${SIPHON_ENDPOINT}/api/capture`, {
      method: "POST",
      headers: bridgeHeaders({ "Content-Type": "application/json" }),
      body: JSON.stringify(payload)
    });
    if (resp.ok) {
      const data = await resp.json();
      return Boolean(data && data.ok && data.captured);
    }
  } catch (_) {
    const nativeReply = await sendNativeToSiphon({
      type: "browser_capture",
      ...payload
    });
    return Boolean(nativeReply && nativeReply.ok);
  }
  return false;
}

async function sendSniffToSiphon(tab, items, bringToFront = true) {
  const pageUrl = (tab && tab.url) || "";
  const cookie = await getCookieHeader(pageUrl);
  const payload = {
    page_title: (tab && tab.title) || null,
    page_url: pageUrl || null,
    referrer: pageUrl || null,
    cookie: cookie || null,
    user_agent: navigator.userAgent,
    items: Array.isArray(items) ? items : [],
    bring_to_front: Boolean(bringToFront)
  };

  try {
    const resp = await fetch(`${SIPHON_ENDPOINT}/api/sniff`, {
      method: "POST",
      headers: bridgeHeaders({ "Content-Type": "application/json" }),
      body: JSON.stringify(payload)
    });
    if (resp.ok) {
      const data = await resp.json();
      return Boolean(data && data.ok);
    }
  } catch (_) {
    const nativeReply = await sendNativeToSiphon({
      type: "browser_sniff",
      ...payload
    });
    return Boolean(nativeReply && nativeReply.ok);
  }
  return false;
}

chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  suggest();
  handleDownloadItem(item, true);
});

chrome.downloads.onCreated.addListener((item) => {
  setTimeout(async () => {
    if (!item || handledIds.has(item.id)) return;
    try {
      const results = await chrome.downloads.search({ id: item.id });
      if (results && results[0]) {
        await handleDownloadItem(results[0], false);
      }
    } catch (_) {
      await handleDownloadItem(item, false);
    }
  }, 350);
});

async function handleDownloadItem(item, fromDeterminingFilename = false) {
  if (!item || handledIds.has(item.id)) return;
  if (item.state && item.state !== "in_progress") return;

  const st = await chrome.storage.local.get({ enabled: true });
  if (!st.enabled) return;

  if (!shouldCaptureDownload(item, fromDeterminingFilename)) return;
  handledIds.add(item.id);

  try {
    await chrome.downloads.pause(item.id);
  } catch (_) {}

  const url = item.finalUrl || item.url;
  const captured = await sendToSiphon({
    url,
    filename: item.filename,
    referrer: item.referrer,
    mime: item.mime,
    fileSize: item.fileSize,
    silent: false
  });

  if (captured) {
    flashBadge("OK", "#22C55E");
    try {
      await chrome.downloads.cancel(item.id);
      await chrome.downloads.erase({ id: item.id });
    } catch (_) {}
  } else {
    handledIds.delete(item.id);
    try {
      await chrome.downloads.resume(item.id);
    } catch (_) {}
  }
}
