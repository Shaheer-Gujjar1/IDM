const PORT = 9600;

document.addEventListener('DOMContentLoaded', async () => {
  const statusDot = document.getElementById('status-dot');
  const statusText = document.getElementById('status-text');
  const btnAddCurrent = document.getElementById('btn-add-manually');
  const powerCard = document.getElementById('power-card');
  const powerStatus = document.getElementById('power-status');
  const powerToggleBtn = document.getElementById('power-toggle-btn');

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

  // Update power toggle UI
  function updatePowerUI(enabled) {
    if (enabled) {
      powerCard.classList.remove('disabled');
      powerStatus.textContent = 'Active';
      powerStatus.style.color = 'var(--power-status-active)';
      btnAddCurrent.disabled = false;
      btnAddCurrent.style.opacity = '1';
      btnAddCurrent.style.cursor = 'pointer';
    } else {
      powerCard.classList.add('disabled');
      powerStatus.textContent = 'Paused Temporary';
      powerStatus.style.color = 'var(--text-secondary)';
      btnAddCurrent.disabled = true;
      btnAddCurrent.style.opacity = '0.5';
      btnAddCurrent.style.cursor = 'not-allowed';
    }
  }

  // Bind click toggle
  powerToggleBtn.addEventListener('click', async () => {
    const current = await chrome.storage.local.get('extensionEnabled');
    const newState = current.extensionEnabled === false ? true : false;
    await chrome.storage.local.set({ extensionEnabled: newState });
    updatePowerUI(newState);
  });

  // 2. Send current tab directly
  btnAddCurrent.addEventListener('click', async () => {
    const state = await chrome.storage.local.get('extensionEnabled');
    if (state.extensionEnabled === false) return;

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
        btnAddCurrent.textContent = 'Success!';
        btnAddCurrent.style.background = '#10b981';
      } else {
        btnAddCurrent.textContent = 'Failed to Send';
        btnAddCurrent.style.background = '#ef4444';
        setTimeout(() => {
          btnAddCurrent.textContent = 'Download this page as HTML';
          // Check state again when enabling
          chrome.storage.local.get('extensionEnabled', (data) => {
            const isEnabled = data.extensionEnabled !== false;
            btnAddCurrent.disabled = !isEnabled;
            btnAddCurrent.style.background = '';
          });
        }, 2000);
      }
    });
  });

  // Run checks
  const initialState = await chrome.storage.local.get('extensionEnabled');
  updatePowerUI(initialState.extensionEnabled !== false);
  await checkAppConnection();
});
