// Service worker for Impressive Download Manager Extension - Phase 5 (HTTPS Hero Engine)
const PORT = 9600;

let extensionEnabled = true;
const handledDownloadIds = new Set();
const webRequestCapturedUrls = new Map(); // filename -> direct HTTPS URL

// Initialize state
chrome.storage.local.get("extensionEnabled", (data) => {
  if (data.extensionEnabled !== undefined) {
    extensionEnabled = data.extensionEnabled;
  } else {
    chrome.storage.local.set({ extensionEnabled: true });
  }
});

// Watch changes
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'local' && changes.extensionEnabled) {
    extensionEnabled = changes.extensionEnabled.newValue;
  }
});

// Reset state on startup/install
chrome.runtime.onStartup.addListener(() => {
  chrome.storage.local.set({ extensionEnabled: true });
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({ extensionEnabled: true });
});

// Helper to extract cookie headers for a target URL
async function getCookiesForUrl(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map(c => `${c.name}=${c.value}`).join('; ');
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
        const filename = u.substring(u.lastIndexOf('/') + 1).split('?')[0];
        if (filename && filename.length > 3) {
          webRequestCapturedUrls.set(filename, u);
          console.log("[HTTPS Direct WebRequest Signal]", filename, "->", u);
        }
      },
      { urls: ["*://downloads.sourceforge.net/*", "*://*.dl.sourceforge.net/*", "*://objects.githubusercontent.com/*"] }
    );
  } catch (e) {
    console.warn("webRequest listener failed to attach:", e);
  }
}

// 2. HTTPS Download Detection Pipeline
function checkAndProcessDownload(id) {
  if (!extensionEnabled || handledDownloadIds.has(id)) {
    return;
  }

  let attempts = 0;
  const maxAttempts = 20; // 10s poll window

  const interval = setInterval(async () => {
    attempts++;
    chrome.downloads.search({ id }, async (items) => {
      if (!items || items.length === 0) {
        clearInterval(interval);
        return;
      }

      const item = items[0];
      if (!item || item.state === "interrupted" || item.state === "complete") {
        clearInterval(interval);
        return;
      }

      const url = item.url || "";
      const finalUrl = item.finalUrl || "";
      const mime = item.mime || "";
      const filenameStr = item.filename ? item.filename.replace(/^.*[\\\/]/, '') : "";

      // Check direct webRequest mirror signal
      const directUrlSignal = filenameStr ? webRequestCapturedUrls.get(filenameStr) : null;
      const targetRealUrl = directUrlSignal || finalUrl;

      // Reject HTML web pages and redirector landing URLs strictly
      if (mime === "text/html" || mime === "text/plain" || mime.includes("html")) {
        return;
      }
      // Use URL parsing to strictly reject SourceForge landing pages even with query strings
      try {
        const pUrl = new URL(targetRealUrl);
        if (pUrl.hostname.includes("sourceforge.net") && pUrl.pathname.endsWith("/download")) {
          return;
        }
      } catch(e) {}

      const isBinaryExt = /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage)$/i.test(filenameStr) || /\.(run|exe|zip|deb|dmg|msi|tar|gz|iso|apk|appimage)$/i.test(url);
      const isNonTextMime = mime && !mime.startsWith("text/");
      const hasSize = item.fileSize && item.fileSize > 0;
      const hasFinalUrl = targetRealUrl && targetRealUrl !== url;

      const isRealFile = (hasSize || isNonTextMime || isBinaryExt) && (hasFinalUrl || directUrlSignal);

      if (isRealFile && !handledDownloadIds.has(id)) {
        handledDownloadIds.add(id);
        clearInterval(interval);

        const cleanFilename = filenameStr || extractFilename(targetRealUrl);
        const cookies = await getCookiesForUrl(targetRealUrl);
        let referrer = item.referrer || "";
        if (!referrer) {
          try {
            const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
            if (tabs && tabs[0]) {
              referrer = tabs[0].url || "";
            }
          } catch (e) {}
        }

        const payload = {
          url: targetRealUrl,
          filename: cleanFilename,
          size: item.fileSize && item.fileSize > 0 ? item.fileSize : 0,
          mime: mime,
          referrer: referrer,
          cookie: cookies,
          userAgent: navigator.userAgent
        };

        console.log("HTTPS HERO: CONFIRMED REAL FILE PAYLOAD SENT TO TAURI BACKEND:", payload);

        // 3. Engine Handoff: Send payload directly to Tauri backend
        const success = await sendToDesktopApp(payload);

        if (success) {
          // 4. Browser Cancellation: ONLY AFTER backend acknowledges receipt
          try {
            chrome.downloads.cancel(id, () => {
              console.log("Cancelled browser download AFTER successful handoff:", id);
            });
          } catch (e) {}
        } else {
          // 5. Failsafe: If backend fails/offline, resume browser download
          console.warn("Backend handoff failed. Failsafe active: resuming browser download", id);
          try {
            chrome.downloads.resume(id, () => {});
          } catch (e) {}
        }
        return;
      }

      if (attempts >= maxAttempts) {
        clearInterval(interval);
        console.log("No real HTTPS file URL confirmed within 10s. Failsafe: Leaving browser download running.");
        try {
          chrome.downloads.resume(id, () => {});
        } catch (e) {}
      }
    });
  }, 500);
}

chrome.downloads.onCreated.addListener((item) => {
  if (item && item.id) {
    checkAndProcessDownload(item.id);
  }
});

chrome.downloads.onChanged.addListener((delta) => {
  if (delta && delta.id) {
    checkAndProcessDownload(delta.id);
  }
});

const recentInterceptions = new Map();

// Helper to send payloads to Tauri backend (port 9600)
async function sendToDesktopApp(payload) {
  const url = payload.url;
  const now = Date.now();
  const lastIntercept = recentInterceptions.get(url);
  if (lastIntercept && (now - lastIntercept) < 2500) {
    console.log("De-duplicated download payload for URL:", url);
    return false;
  }

  for (const [key, time] of recentInterceptions.entries()) {
    if (now - time > 15000) {
      recentInterceptions.delete(key);
    }
  }

  for (let attempt = 1; attempt <= 2; attempt++) {
    try {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), 1500);

      const response = await fetch(`http://127.0.0.1:${PORT}/download`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        mode: 'cors',
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
    } catch (err) {
      if (attempt === 1) {
        console.warn("Tauri backend offline on port 9600. Trying wakeup...");
        try {
          chrome.tabs.create({ url: "idm://wakeup", active: false }, (tab) => {
            if (tab && tab.id) {
              setTimeout(() => {
                chrome.tabs.remove(tab.id, () => {});
              }, 400);
            }
          });
        } catch (e) {}
        await new Promise(r => setTimeout(r, 800));
      }
    }
  }
  return false;
}

function extractFilename(url) {
  try {
    const pathname = new URL(url).pathname;
    const last = pathname.substring(pathname.lastIndexOf("/") + 1).split('?')[0];
    return last || "captured_download";
  } catch {
    return "captured_download";
  }
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'SEND_TO_DESKTOP') {
    if (!extensionEnabled) {
      sendResponse({ success: false });
      return true;
    }
    (async () => {
      const cookies = await getCookiesForUrl(message.url);
      const payload = {
        url: message.url,
        filename: message.filename || extractFilename(message.url),
        size: 0,
        mime: "",
        referrer: "",
        cookie: cookies,
        userAgent: navigator.userAgent
      };
      const success = await sendToDesktopApp(payload);
      sendResponse({ success });
    })();
    return true;
  }
});
