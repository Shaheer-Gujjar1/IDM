// Service worker for Impressive Download Manager Extension
const PORT = 9600;

// Setup context menus
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "send-to-impressive-dm",
    title: "Send Link to Impressive DM",
    contexts: ["link"]
  });
});

// Intercept browser downloads automatically
chrome.downloads.onCreated.addListener(async (downloadItem) => {
  // Check if download URL is valid and not triggered by our own extension
  if (downloadItem.url && !downloadItem.byExtensionId) {
    try {
      // Cancel and remove the default browser download
      await chrome.downloads.cancel(downloadItem.id);
      await chrome.downloads.erase({ id: downloadItem.id });
    } catch (e) {
      console.warn("Interception cancel failed:", e);
    }

    const filename = downloadItem.filename 
      ? downloadItem.filename.split(/[\\/]/).pop() 
      : extractFilename(downloadItem.url);
    
    await sendToDesktopApp(downloadItem.url, filename);
  }
});

// Context Menu action listener
chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === "send-to-impressive-dm" && info.linkUrl) {
    const filename = extractFilename(info.linkUrl);
    await sendToDesktopApp(info.linkUrl, filename);
  }
});

// Listen to media sniffing packets from content script
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'MEDIA_DETECTED') {
    (async () => {
      const tabId = sender.tab?.id;
      if (!tabId) return;

      const storeKey = `captured_tab_${tabId}`;
      const data = await chrome.storage.local.get(storeKey);
      const existing = data[storeKey] || [];

      // Avoid duplicates
      const updated = [...existing];
      message.urls.forEach(url => {
        if (!updated.some(item => item.url === url)) {
          updated.push({
            url,
            filename: extractFilename(url),
            timestamp: Date.now()
          });
        }
      });

      await chrome.storage.local.set({ [storeKey]: updated });
      
      // Update browser action badge count
      if (updated.length > 0) {
        await chrome.action.setBadgeText({
          tabId: tabId,
          text: String(updated.length)
        });
        await chrome.action.setBadgeBackgroundColor({
          tabId: tabId,
          color: "#00f0ff"
        });
      }
    })();
  }
  return true;
});

// Helper to send payloads to our Tauri backend port 9600
async function sendToDesktopApp(url, filename) {
  try {
    const response = await fetch(`http://127.0.0.1:${PORT}/download`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      mode: 'cors',
      body: JSON.stringify({ url, filename })
    });
    const result = await response.json();
    console.log("Desktop app response:", result);
    return true;
  } catch (err) {
    console.error("Failed to connect to Impressive Download Manager app:", err);
    return false;
  }
}

// Extractor helper
function extractFilename(url) {
  try {
    const pathname = new URL(url).pathname;
    const last = pathname.substring(pathname.lastIndexOf("/") + 1);
    return last || "captured_download";
  } catch {
    return "captured_download";
  }
}

// Export sending logic for popup
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'SEND_TO_DESKTOP') {
    sendToDesktopApp(message.url, message.filename).then(success => {
      sendResponse({ success });
    });
    return true;
  }
});
