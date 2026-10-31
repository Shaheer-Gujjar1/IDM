// Service worker for Impressive Download Manager Extension
if (typeof importScripts === "function") {
  try {
    importScripts("hls.js", "media-choices.js", "youtube-media.js");
  } catch (e) {}
}

const PORT = 9600;

function isFragment(url) {
  return /\.(m4s|ts|aac|vtt|webvtt)(?:[?#]|$)/i.test(url) || /\/init[^/]*\.mp4(?:[?#]|$)/i.test(url);
}

function isVideoDownload(filename, mime, url, targetUrl) {
  if (mime && (mime.startsWith("video/") || mime === "application/x-mpegURL" || mime === "application/vnd.apple.mpegurl")) {
    return true;
  }
  const videoExtRegex = /\.(mp4|mkv|webm|mov|avi|flv|wmv|m4v|3gp|ts|m3u8|f4v|vob|ogv|mpg|mpeg)($|\?)/i;
  return videoExtRegex.test(filename || "") || videoExtRegex.test(url || "") || (Boolean(targetUrl) && videoExtRegex.test(targetUrl));
}

let extensionEnabled = true;
let desktopAppInterceptionDisabled = false;
const handledDownloadIds = new Set();
const webRequestCapturedUrls = new Map(); // filename -> direct HTTPS URL
const capturedMediaByTab = new Map(); // tabId -> media items
const youtubeDataByTab = new Map(); // tabId -> sanitized player response
const observedVideoUrlsByTab = new Map(); // tabId -> list of videoplayback URLs

function rememberMediaForTab(tabId, items) {
  if (!tabId || !Array.isArray(items)) return;
  const existing = capturedMediaByTab.get(tabId) || [];
  const existingUrls = new Set(existing.map(i => i.url));
  for (const item of items) {
    if (item && item.url && !existingUrls.has(item.url)) {
      if (isFragment(item.url)) continue;
      existingUrls.add(item.url);
      existing.unshift(item);
    }
  }
  if (existing.length > 50) existing.length = 50;
  capturedMediaByTab.set(tabId, existing);
}

const tabVideoIdMap = new Map();

chrome.tabs.onRemoved?.addListener((tabId) => {
  capturedMediaByTab.delete(tabId);
  youtubeDataByTab.delete(tabId);
  observedVideoUrlsByTab.delete(tabId);
  tabVideoIdMap.delete(tabId);
});

chrome.tabs.onUpdated?.addListener((tabId, changeInfo) => {
  if (changeInfo.url) {
    try {
      const u = new URL(changeInfo.url);
      const newV = u.searchParams.get('v');
      const oldV = tabVideoIdMap.get(tabId);
      if (newV && oldV && newV !== oldV) {
        capturedMediaByTab.delete(tabId);
        youtubeDataByTab.delete(tabId);
        observedVideoUrlsByTab.delete(tabId);
        tabVideoIdMap.set(tabId, newV);
      } else if (newV) {
        tabVideoIdMap.set(tabId, newV);
      }
    } catch {}
  }
});

let currentAppTheme = "dark";

function broadcastTheme(theme) {
  try {
    chrome.tabs.query({}, (tabs) => {
      for (const tab of tabs || []) {
        if (tab.id) {
          chrome.tabs.sendMessage(tab.id, { action: "themeChanged", theme }).catch(() => {});
        }
      }
    });
  } catch (e) {}
}

// Check desktop app interception setting and theme status
async function checkDesktopAppStatus() {
  try {
    const res = await fetch(`http://127.0.0.1:${PORT}/status`, { cache: "no-store" });
    if (res.ok) {
      const data = await res.json();
      desktopAppInterceptionDisabled = data.enabled === false;
      if (data.theme && data.theme !== currentAppTheme) {
        currentAppTheme = data.theme;
        chrome.storage.local.set({ appTheme: currentAppTheme });
        broadcastTheme(currentAppTheme);
      }
    } else if (res.status === 403) {
      desktopAppInterceptionDisabled = true;
    }
  } catch (e) {
    // If backend is offline, leave extensionEnabled behavior intact
  }
}
setInterval(checkDesktopAppStatus, 2000);
checkDesktopAppStatus();

// Initialize state
chrome.storage.local.get(["extensionEnabled", "appTheme"], (data) => {
  if (data.extensionEnabled !== undefined) {
    extensionEnabled = data.extensionEnabled;
  } else {
    chrome.storage.local.set({ extensionEnabled: true });
  }
  if (data.appTheme) {
    currentAppTheme = data.appTheme;
  }
});

// Watch changes
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local") {
    if (changes.extensionEnabled) {
      extensionEnabled = changes.extensionEnabled.newValue;
    }
    if (changes.appTheme) {
      currentAppTheme = changes.appTheme.newValue;
      broadcastTheme(currentAppTheme);
    }
  }
});

// Reset state on startup/install
chrome.runtime.onStartup.addListener(() => {
  chrome.storage.local.set({ extensionEnabled: true });
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({ extensionEnabled: true });
});

// Helper to extract cookie headers for a target URL and active tab
async function getCookiesForUrl(url, tabUrl) {
  try {
    const cookieMap = new Map();

    // 1. Get cookies for target download URL
    if (url && (url.startsWith("http://") || url.startsWith("https://"))) {
      try {
        const cookies1 = await chrome.cookies.getAll({ url });
        if (cookies1) {
          for (const c of cookies1) {
            cookieMap.set(c.name, c.value);
          }
        }
      } catch (e) {}

      // Also get cookies for root domain if subdomain
      try {
        const u = new URL(url);
        const parts = u.hostname.split(".");
        if (parts.length > 2) {
          const domain = parts.slice(-2).join(".");
          const domainCookies = await chrome.cookies.getAll({ domain });
          if (domainCookies) {
            for (const c of domainCookies) {
              if (!cookieMap.has(c.name)) cookieMap.set(c.name, c.value);
            }
          }
        }
      } catch (e) {}
    }

    // 2. Also get cookies from active tab URL if different
    if (tabUrl && (tabUrl.startsWith("http://") || tabUrl.startsWith("https://"))) {
      try {
        const cookies2 = await chrome.cookies.getAll({ url: tabUrl });
        if (cookies2) {
          for (const c of cookies2) {
            if (!cookieMap.has(c.name)) {
              cookieMap.set(c.name, c.value);
            }
          }
        }
      } catch (e) {}
    }

    const merged = Array.from(cookieMap.entries())
      .map(([name, value]) => `${name}=${value}`)
      .join("; ");
    return merged;
  } catch (err) {
    console.warn("Failed to get cookies:", err);
    return "";
  }
}

// 1. Non-blocking webRequest listener for HTTPS direct mirror signals
if (chrome.webRequest && chrome.webRequest.onBeforeRequest) {
  try {
    chrome.webRequest.onBeforeRequest.addListener(
      (details) => {
        if (!details.url) return;
        const u = details.url;
        const filename = u.substring(u.lastIndexOf("/") + 1).split("?")[0];
        if (filename && filename.length > 3) {
          webRequestCapturedUrls.set(filename, u);
          console.log("[HTTPS Direct WebRequest Signal]", filename, "->", u);
        }
      },
      {
        urls: [
          "*://downloads.sourceforge.net/*",
          "*://*.dl.sourceforge.net/*",
          "*://objects.githubusercontent.com/*"
        ]
      }
    );
  } catch (e) {
    console.warn("webRequest listener failed to attach:", e);
  }
}

// 1.1 Record active request headers for captured media URLs
const requestHeadersByTab = new Map();
function rememberRequestHeaders(details) {
  if (details.tabId < 0 || !details.url || !details.url.startsWith("http")) return;
  let tab = requestHeadersByTab.get(details.tabId);
  if (!tab) {
    tab = new Map();
    requestHeadersByTab.set(details.tabId, tab);
    if (requestHeadersByTab.size > 50) requestHeadersByTab.delete(requestHeadersByTab.keys().next().value);
  }
  const headers = {};
  for (const h of details.requestHeaders || []) {
    const name = h.name.toLowerCase();
    if (['cookie', 'authorization', 'user-agent', 'referer', 'origin'].includes(name) && typeof h.value === 'string' && h.value.length <= 16384) {
      headers[name] = h.value;
    }
  }
  tab.set(details.url, headers);
  if (tab.size > 100) tab.delete(tab.keys().next().value);
}
if (chrome.webRequest && chrome.webRequest.onBeforeSendHeaders) {
  try {
    chrome.webRequest.onBeforeSendHeaders.addListener(rememberRequestHeaders, { urls: ["http://*/*", "https://*/*"] }, ["requestHeaders", "extraHeaders"]);
  } catch (e) {
    try {
      chrome.webRequest.onBeforeSendHeaders.addListener(rememberRequestHeaders, { urls: ["http://*/*", "https://*/*"] }, ["requestHeaders"]);
    } catch {}
  }
}

// 1.2 Detect streaming playlists and active media URLs in verified network responses
if (chrome.webRequest && chrome.webRequest.onHeadersReceived) {
  try {
    chrome.webRequest.onHeadersReceived.addListener(
      (details) => {
        if (!details.url || details.tabId < 0) return;
        const mime = details.responseHeaders?.find(h => h.name.toLowerCase() === 'content-type')?.value || '';
        const isSuccess = [200, 206].includes(details.statusCode);

        // 1. YouTube media chunks: ONLY capture if HTTP 200/206 and content-type is video/audio
        if (details.url.includes("googlevideo.com/videoplayback") && isSuccess && /^(video|audio)\//i.test(mime)) {
          const captured = globalThis.youtubeMedia?.mediaUrl(details.url);
          if (captured) {
            const list = observedVideoUrlsByTab.get(details.tabId) || [];
            if (!list.includes(captured)) {
              list.unshift(captured);
              if (list.length > 50) list.pop();
              observedVideoUrlsByTab.set(details.tabId, list);
            }
          }
        }

        // 2. Direct media files
        if (isSuccess && /^(video|audio)\//i.test(mime) && !isFragment(details.url)) {
          (async () => {
            let title = "";
            try {
              const t = await chrome.tabs.get(details.tabId);
              title = t?.title || "";
            } catch {}
            rememberMediaForTab(details.tabId, [{ url: details.url, title, format: mime.includes("audio") ? "MP3" : "MP4" }]);
          })();
        }

        // 3. HLS master playlists
        if (/\.m3u8(?:[?#]|$)/i.test(details.url) || /mpegurl/i.test(mime)) {
          (async () => {
            try {
              const resp = await fetch(details.url, { credentials: "omit" });
              if (resp.ok) {
                const text = await resp.text();
                if (globalThis.parseHlsMaster) {
                  const variants = globalThis.parseHlsMaster(text, resp.url || details.url);
                  let title = "";
                  try {
                    const t = await chrome.tabs.get(details.tabId);
                    title = t?.title || "";
                  } catch {}
                  rememberMediaForTab(details.tabId, variants.map(v => ({ ...v, title })));
                }
              }
            } catch {}
          })();
        }
      },
      { urls: ["http://*/*", "https://*/*"] },
      ["responseHeaders"]
    );
  } catch (e) {
    console.warn("webRequest onHeadersReceived listener failed to attach:", e);
  }
}

// 2. HTTPS Download Detection Pipeline (Instant Interception via onDeterminingFilename)
async function interceptDownloadItem(item) {
  if (!item || !extensionEnabled || desktopAppInterceptionDisabled || handledDownloadIds.has(item.id)) {
    return false;
  }
  if (item.state === "interrupted" || item.state === "complete") {
    return false;
  }

  const url = item.url || "";
  const finalUrl = item.finalUrl || url;

  // Reject non-HTTP(S) protocols (blob:, data:, file:, chrome:) so the browser handles in-memory objects natively
  if (!url.startsWith("http://") && !url.startsWith("https://") && !finalUrl.startsWith("http://") && !finalUrl.startsWith("https://")) {
    console.log("[Extension] In-memory or non-HTTP(S) protocol (e.g. blob:, data:). Letting browser save natively:", url);
    return false;
  }

  const mime = item.mime || "";
  const filenameStr = item.filename ? item.filename.replace(/^.*[\\\/]/, "") : "";
  const fileSize = (item.fileSize && item.fileSize > 0) ? item.fileSize : (item.totalBytes && item.totalBytes > 0 ? item.totalBytes : 0);

  // Check direct webRequest mirror signal
  const directUrlSignal = filenameStr ? webRequestCapturedUrls.get(filenameStr) : null;
  let targetRealUrl = directUrlSignal || finalUrl;
  if ((url.includes("sourceforge.net") || targetRealUrl.includes("sourceforge.net")) && url.includes("/project/")) {
    targetRealUrl = url;
  }

  // Reject streaming media chunks / fragments (.m4s, .ts, .aac, .vtt)
  if (isFragment(url) || isFragment(targetRealUrl) || isFragment(finalUrl)) {
    console.log("[Extension] Ignored streaming media chunk fragment:", targetRealUrl);
    return false;
  }

  // Reject HTML web pages and redirector landing URLs strictly
  if (mime === "text/html" || mime === "text/plain" || mime.includes("html")) {
    return false;
  }

  // Use URL parsing to strictly reject SourceForge landing pages even with query strings
  try {
    const pUrl = new URL(targetRealUrl);
    if (pUrl.hostname.includes("sourceforge.net") && pUrl.pathname.endsWith("/download")) {
      return false;
    }
  } catch (e) {}

  const cleanFilename = filenameStr || extractFilename(targetRealUrl);
  const isVideo = isVideoDownload(cleanFilename, mime, url, targetRealUrl);

  // Files with size < 1.01 MB (1,059,062 bytes) are left for the browser to download natively,
  // except for video files which are ALWAYS captured regardless of size.
  const MIN_NON_VIDEO_SIZE = Math.round(1.01 * 1024 * 1024); // 1,059,062 bytes (~1.01 MB)
  if (!isVideo && fileSize > 0 && fileSize < MIN_NON_VIDEO_SIZE) {
    console.log(`[Extension] Non-video file size (${fileSize}B < 1.01MB). Leaving to browser natively:`, cleanFilename);
    return false;
  }

  const isBinaryExt =
    /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage|7z|rar|bz2|xz|pdf|docx?|xlsx?|pptx?|mp4|mkv|webm|mp3|wav|flac|bin|pkg|csv|epub|mobi)$/i.test(cleanFilename) ||
    /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage|7z|rar|bz2|xz|pdf|docx?|xlsx?|pptx?|mp4|mkv|webm|mp3|wav|flac|bin|pkg|csv|epub|mobi)$/i.test(url) ||
    /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage|7z|rar|bz2|xz|pdf|docx?|xlsx?|pptx?|mp4|mkv|webm|mp3|wav|flac|bin|pkg|csv|epub|mobi)$/i.test(targetRealUrl);
  const isNonTextMime = mime && !mime.startsWith("text/");
  const hasSubstantialSize = fileSize >= 1024 * 1024;
  const hasDownloadExt = /\.[a-z0-9]{2,8}$/i.test(cleanFilename) && !/\.(html?|php|asp|jsp|cgi)$/i.test(cleanFilename);

  const isRealFile = (hasSubstantialSize || isNonTextMime || isBinaryExt || hasDownloadExt);

  if (isRealFile) {
    let tabUrl = "";
    let referrer = item.referrer || "";
    try {
      const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
      if (tabs && tabs[0]) {
        tabUrl = tabs[0].url || "";
        if (!referrer) {
          referrer = tabUrl;
        }
      }
    } catch (e) {}

    const cookies = await getCookiesForUrl(targetRealUrl, tabUrl);

    const payload = {
      url: targetRealUrl,
      filename: cleanFilename,
      size: fileSize,
      mime: mime,
      referrer: referrer,
      cookie: cookies,
      userAgent: navigator.userAgent
    };

    console.log("HTTPS HERO: ATTEMPTING DESKTOP CAPTURE:", payload);
    const accepted = await sendToDesktopApp(payload);

    if (accepted) {
      handledDownloadIds.add(item.id);
      // Cancel & erase browser download ONLY AFTER IDM confirms receipt!
      try {
        chrome.downloads.cancel(item.id, () => {
          try {
            chrome.downloads.erase({ id: item.id }, () => {});
          } catch (e) {}
        });
      } catch (e) {}
      return true;
    }

    console.warn("Desktop rejected or offline; preserving native browser download.");
    return false;
  }
  return false;
}

if (chrome.downloads && chrome.downloads.onDeterminingFilename) {
  chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
    (async () => {
      try {
        const intercepted = await interceptDownloadItem(item);
        if (!intercepted && typeof suggest === "function") {
          suggest();
        }
      } catch (err) {
        console.error("[Extension] Error during download interception:", err);
        if (typeof suggest === "function") {
          suggest();
        }
      }
    })();
    return true;
  });
}

const recentInterceptions = new Map();
const NATIVE_HOST = "com.impressive.idm";

// Helper to send payloads via Chrome/Firefox Native Messaging Host
function sendViaNativeHost(payload) {
  return new Promise((resolve, reject) => {
    try {
      if (typeof chrome !== "undefined" && chrome.runtime && typeof chrome.runtime.sendNativeMessage === "function") {
        chrome.runtime.sendNativeMessage(NATIVE_HOST, payload, (response) => {
          if (chrome.runtime.lastError) {
            reject(new Error(chrome.runtime.lastError.message));
          } else if (response && response.ok) {
            resolve(response);
          } else {
            reject(new Error(response?.error || "Native messaging host returned error"));
          }
        });
      } else if (typeof browser !== "undefined" && browser.runtime && typeof browser.runtime.sendNativeMessage === "function") {
        browser.runtime.sendNativeMessage(NATIVE_HOST, payload)
          .then((res) => {
            if (res && res.ok) resolve(res);
            else reject(new Error(res?.error || "Native messaging host returned error"));
          })
          .catch(reject);
      } else {
        reject(new Error("Native messaging API unavailable"));
      }
    } catch (e) {
      reject(e);
    }
  });
}

// Helper to send payloads to Tauri backend (fast HTTP daemon first, Native Messaging fallback)
async function sendToDesktopApp(payload) {
  const url = payload.url;
  const now = Date.now();
  const lastIntercept = recentInterceptions.get(url);
  if (lastIntercept && now - lastIntercept < 2500) {
    console.log("De-duplicated download payload for URL:", url);
    return false;
  }

  for (const [key, time] of recentInterceptions.entries()) {
    if (now - time > 15000) {
      recentInterceptions.delete(key);
    }
  }

  // 1. Fast path: Direct HTTP port 9600 (instant < 2ms when IDM is running)
  for (let attempt = 1; attempt <= 2; attempt++) {
    try {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), 1200);

      const response = await fetch(`http://127.0.0.1:${PORT}/download`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        },
        mode: "cors",
        signal: controller.signal,
        body: JSON.stringify(payload)
      });
      clearTimeout(timeoutId);

      if (response.ok) {
        recentInterceptions.set(url, now);
        const result = await response.json();
        console.log("Tauri backend response:", result);
        return true;
      } else if (response.status === 403) {
        console.log("Download interception is toggled OFF in desktop app settings.");
        return false;
      }
    } catch (httpErr) {
      // If port 9600 failed on first attempt, try native messaging or protocol wakeup before retry
      if (attempt === 1) {
        console.info("Port 9600 unreachable (" + httpErr.message + "). Trying Native Messaging Host...");
        try {
          const nativeResult = await sendViaNativeHost(payload);
          if (nativeResult && nativeResult.ok) {
            recentInterceptions.set(url, now);
            console.log("Native messaging host successfully accepted download:", nativeResult);
            return true;
          }
        } catch (nativeErr) {
          console.info("Native messaging host inactive (" + nativeErr.message + "). Trying protocol wakeup...");
          try {
            chrome.tabs.create({ url: "idm://wakeup", active: false }, (tab) => {
              if (tab && tab.id) {
                setTimeout(() => {
                  chrome.tabs.remove(tab.id, () => { });
                }, 400);
              }
            });
          } catch (e) { }
          await new Promise((r) => setTimeout(r, 600));
        }
      }
    }
  }
  return false;
}


function extractFilename(url) {
  try {
    const parsed = new URL(url);
    // Check if filename is present in query parameters (e.g. ?filename=..., ?file=..., ?name=...)
    for (const [key, val] of parsed.searchParams.entries()) {
      const k = key.toLowerCase();
      if ((k.includes("file") || k.includes("name") || k.includes("title")) && val.includes(".")) {
        const seg = val.split("/").pop()?.split("\\").pop();
        if (seg && seg.includes(".")) {
          try {
            return decodeURIComponent(seg);
          } catch {
            return seg;
          }
        }
      }
    }

    const pathname = parsed.pathname;
    const last = pathname.substring(pathname.lastIndexOf("/") + 1).split("?")[0];
    if (last && last !== "download") {
      try {
        return decodeURIComponent(last);
      } catch {
        return last;
      }
    }
    return "captured_download";
  } catch {
    return "captured_download";
  }
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  const tabId = sender?.tab?.id;

  if (message.action === "playerResponse" && tabId) {
    if (globalThis.youtubeMedia) {
      const clean = globalThis.youtubeMedia.playerData(message.data);
      if (clean) {
        youtubeDataByTab.set(tabId, clean);
        const observed = observedVideoUrlsByTab.get(tabId) || [];
        const choices = globalThis.youtubeMedia.choices(clean, observed);
        if (choices.length > 0) {
          rememberMediaForTab(tabId, choices);
        }
      }
    }
    sendResponse({ ok: true });
    return true;
  }

  if (message.action === "remember") {
    if (tabId && Array.isArray(message.items)) {
      rememberMediaForTab(tabId, message.items);
    }
    sendResponse({ ok: true });
    return true;
  }

  if (message.action === "listForFrame") {
    let list = (tabId ? capturedMediaByTab.get(tabId) : []) || [];
    if (tabId && youtubeDataByTab.has(tabId) && globalThis.youtubeMedia) {
      const clean = youtubeDataByTab.get(tabId);
      const observed = observedVideoUrlsByTab.get(tabId) || [];
      const ytChoices = globalThis.youtubeMedia.choices(clean, observed);
      if (ytChoices.length > 0) {
        list = [...ytChoices, ...list.filter(i => i.provider !== "youtube")];
      }
    }
    sendResponse({ ok: true, items: list });
    return true;
  }

  if (message.action === "getTheme") {
    sendResponse({ ok: true, theme: currentAppTheme });
    return true;
  }

  if (message.action === "download" || message.type === "SEND_TO_DESKTOP") {
    if (!extensionEnabled) {
      sendResponse({ ok: false, success: false, error: "Extension is disabled." });
      return true;
    }
    const item = message.item || message;
    (async () => {
      try {
        const targetUrl = item.url;
        const tabUrl = sender?.tab?.url || item.pageUrl || "";
        const isYt = (targetUrl && (targetUrl.includes("googlevideo.com") || targetUrl.includes("youtube.com"))) || (tabUrl && tabUrl.includes("youtube.com"));
        const bestReferrer = (isYt && (item.pageUrl || tabUrl)) ? (item.pageUrl || tabUrl) : (item.referrer || tabUrl || "");
        const cookies = await getCookiesForUrl(targetUrl, tabUrl);
        const payload = {
          url: targetUrl,
          audio_url: item.audioUrl || undefined,
          filename: item.filename || extractFilename(targetUrl),
          size: item.size || 0,
          mime: item.mime || "",
          referrer: bestReferrer,
          page_url: item.pageUrl || tabUrl || undefined,
          video_id: item.videoId || undefined,
          quality: item.quality || undefined,
          cookie: cookies || "",
          userAgent: navigator.userAgent
        };
        const ok = await sendToDesktopApp(payload);
        sendResponse({ ok, success: ok });
      } catch (err) {
        console.error("Error in download message handler:", err);
        sendResponse({ ok: false, success: false, error: err.message });
      }
    })();
    return true;
  }
});
