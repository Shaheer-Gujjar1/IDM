const PORT = 9600;

document.addEventListener('DOMContentLoaded', async () => {
  const statusDot = document.getElementById('status-dot');
  const statusText = document.getElementById('status-text');
  const linkList = document.getElementById('link-list');
  const btnAddCurrent = document.getElementById('btn-add-manually');

  // 1. Ping the Tauri App on port 9600 to check connection status and sync theme
  async function checkAppConnection() {
    try {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), 1200);

      // Simple preflight OPTIONS ping
      const response = await fetch(`http://127.0.0.1:${PORT}/download`, {
        method: 'OPTIONS',
        mode: 'cors',
        signal: controller.signal
      });

      clearTimeout(timeoutId);
      statusDot.className = 'status-dot connected';
      statusText.textContent = 'App Connected';

      // Dynamically sync theme using X-App-Theme header returned by backend OPTIONS server
      const appTheme = response.headers.get('X-App-Theme');
      if (appTheme === 'light' || appTheme === 'dark') {
        document.documentElement.setAttribute('data-theme', appTheme);
      }
    } catch (err) {
      statusDot.className = 'status-dot';
      statusText.textContent = 'App Offline';
    }
  }

  // 2. Fetch and list captured items for current tab
  async function loadCapturedLinks() {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab) return;

    const storeKey = `captured_tab_${tab.id}`;
    const data = await chrome.storage.local.get(storeKey);
    const links = data[storeKey] || [];

    linkList.innerHTML = '';
    
    if (links.length === 0) {
      linkList.innerHTML = `
        <div class="empty-state">
          No media streams detected yet.<br>Play a video or load a file to capture it.
        </div>
      `;
      return;
    }

    links.forEach((item, index) => {
      const ext = item.filename.split('.').pop()?.toUpperCase() || 'FILE';
      
      const itemEl = document.createElement('div');
      itemEl.className = 'link-item';
      
      itemEl.innerHTML = `
        <div class="link-info">
          <span class="link-name" title="${item.filename}">${item.filename}</span>
          <span class="link-type">${ext} Stream</span>
        </div>
        <button class="btn-send" id="send-btn-${index}">Send</button>
      `;
      
      linkList.appendChild(itemEl);

      // Handle sending individual link
      document.getElementById(`send-btn-${index}`).addEventListener('click', async () => {
        const btn = document.getElementById(`send-btn-${index}`);
        const parent = btn.parentElement;
        
        // Replace button with deepin-style liquid progress
        const progressContainer = document.createElement('div');
        progressContainer.className = 'liquid-progress-container';
        progressContainer.innerHTML = `
          <div class="liquid-fill" id="liquid-${index}"></div>
          <div class="liquid-icon" id="icon-${index}">↑</div>
        `;
        parent.replaceChild(progressContainer, btn);

        const fill = document.getElementById(`liquid-${index}`);
        const icon = document.getElementById(`icon-${index}`);
        
        // Fake progressive loading for visual flair
        let percent = 0;
        const interval = setInterval(() => {
          percent += Math.random() * 20;
          if (percent > 85) percent = 85;
          fill.style.transform = `translateY(${100 - percent}%)`;
        }, 150);

        chrome.runtime.sendMessage({
          type: 'SEND_TO_DESKTOP',
          url: item.url,
          filename: item.filename
        }, (response) => {
          setTimeout(() => {
            clearInterval(interval);
            if (response && response.success) {
              fill.style.transform = `translateY(0%)`;
              fill.style.background = '#10b981';
              icon.textContent = '✓';
            } else {
              fill.style.transform = `translateY(0%)`;
              fill.style.background = '#ef4444';
              icon.textContent = '✕';
              setTimeout(() => {
                parent.replaceChild(btn, progressContainer);
                btn.textContent = 'Retry';
                btn.disabled = false;
              }, 2000);
            }
          }, 600); // Give it a slight artificial delay so the user sees the cool animation
        });
      });
    });
  }

  // 3. Send current tab directly
  btnAddCurrent.addEventListener('click', async () => {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab || !tab.url) return;

    btnAddCurrent.textContent = 'Sending to App...';
    btnAddCurrent.disabled = true;

    const filename = tab.title ? `${tab.title.replace(/[^a-z0-9]/gi, '_').toLowerCase()}.html` : 'page.html';

    chrome.runtime.sendMessage({
      type: 'SEND_TO_DESKTOP',
      url: tab.url,
      filename: filename
    }, (response) => {
      if (response && response.success) {
        btnAddCurrent.textContent = 'Sent Successfully!';
        btnAddCurrent.style.background = '#10b981';
      } else {
        btnAddCurrent.textContent = 'Failed to Send';
        btnAddCurrent.style.background = '#ef4444';
        setTimeout(() => {
          btnAddCurrent.textContent = 'Send Current Tab to App';
          btnAddCurrent.disabled = false;
          btnAddCurrent.style.background = '';
        }, 2000);
      }
    });
  });

  // Run checks
  await checkAppConnection();
  await loadCapturedLinks();
});
