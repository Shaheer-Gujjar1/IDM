import { useState, useEffect, useRef } from "react";
import {
  Download,
  Search,
  Plus,
  Layers,
  Activity,
  CheckCircle2,
  Clock,
  Trash2,
  Film,
  Music,
  FileText,
  Archive,
  Settings,
  Pause,
  Play,
  X,
  File,
  CheckCircle,
  Network,
  Globe,
  Sliders,
  Calendar,
  RefreshCw,
  FolderOpen,
  Sparkles,
  Zap,
  Gauge,
  ShieldCheck,
  ShieldAlert
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { check as checkUpdate } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import "./App.css";

type DownloadStatus = "Queued" | "Downloading" | "Paused" | "Completed" | { Failed: string } | "Trash";

interface DownloadProgress {
  id: string;
  filename: string;
  total_size: number;
  downloaded: number;
  speed: number;
  eta: string;
  status: DownloadStatus;
  url?: string;
  save_path?: string;
  file_exists?: boolean;
  speed_limited?: boolean;
}

interface Category {
  id: string;
  name: string;
  icon: React.ReactNode;
}

const formatBytes = (bytes: number): string => {
  if (!bytes || bytes === 0) return "0 Bytes";
  const k = 1024;
  const sizes = ["Bytes", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
};

function App() {
  // Query routing state
  const [popupMode, setPopupMode] = useState<string | null>(null);
  const [popupUrl, setPopupUrl] = useState("");
  const [popupFilename, setPopupFilename] = useState("");
  const [popupSavePath, setPopupSavePath] = useState("");
  const [popupCookie, setPopupCookie] = useState("");
  const [popupReferrer, setPopupReferrer] = useState("");
  const [popupTaskId, setPopupTaskId] = useState<string | null>(null);
  const [popupProgress, setPopupProgress] = useState<DownloadProgress | null>(null);
  const [popupSize, setPopupSize] = useState("0");

  // Main Dashboard State
  const [activeCategory, setActiveCategory] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [downloads, setDownloads] = useState<DownloadProgress[]>([]);


  // Modal State (Manual Trigger)
  const [showAddModal, setShowAddModal] = useState(false);
  const [inputUrl, setInputUrl] = useState("");
  const [customFilename, setCustomFilename] = useState("");
  const [savePath, setSavePath] = useState(() => localStorage.getItem("default_save_dir") || "");

  // Drawer State
  const [selectedTask, setSelectedTask] = useState<DownloadProgress | null>(null);

  // Speed Limiter Helper
  const calculateLimitBps = (valStr: string, unitStr: string): number => {
    const num = parseFloat(valStr);
    if (isNaN(num) || num <= 0) return 0;
    if (unitStr === "GB") return Math.round(num * 1024 * 1024 * 1024);
    if (unitStr === "MB") return Math.round(num * 1024 * 1024);
    return Math.round(num * 1024); // KB
  };

  // Settings State
  const [defaultSaveDir, setDefaultSaveDir] = useState(() => localStorage.getItem("default_save_dir") || "");
  const [autostart, setAutostart] = useState(() => {
    const saved = localStorage.getItem("autostart");
    return saved !== null ? saved === "true" : true;
  });
  const [minimizeToTray, setMinimizeToTray] = useState(true);
  const [maxChunks, setMaxChunks] = useState(() => {
    const saved = localStorage.getItem("max_chunks");
    return saved ? Math.min(32, Math.max(1, parseInt(saved, 10) || 8)) : 8;
  });
  const [speedLimitEnabled, setSpeedLimitEnabled] = useState(() => localStorage.getItem("speed_limit_enabled") === "true");
  const [speedLimitVal, setSpeedLimitVal] = useState(() => localStorage.getItem("speed_limit_val") || "512");
  const [speedLimitUnit, setSpeedLimitUnit] = useState<"KB" | "MB" | "GB">(() => (localStorage.getItem("speed_limit_unit") as "KB" | "MB" | "GB") || "KB");
  const [interceptDownloads, setInterceptDownloads] = useState(() => {
    const val = localStorage.getItem("intercept_downloads");
    return val !== null ? val === "true" : true;
  });
  const [integrationPort, setIntegrationPort] = useState(9600);

  // Remove Task Modal States
  const [showRemoveConfirm, setShowRemoveConfirm] = useState(false);
  const [taskToRemove, setTaskToRemove] = useState<DownloadProgress | null>(null);
  const [deleteFileFromDisk, setDeleteFileFromDisk] = useState(false);

  // Updater State & Metrics
  const CURRENT_APP_VERSION = "0.6.0";
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [pendingRelaunch, setPendingRelaunch] = useState(false);
  const [isDownloadingUpdate, setIsDownloadingUpdate] = useState(false);
  const [updatedFromVersion, setUpdatedFromVersion] = useState<string | null>(null);
  const [showUpdateSuccessModal, setShowUpdateSuccessModal] = useState(false);

  const [updateProgressInfo, setUpdateProgressInfo] = useState<{
    downloaded: number;
    total: number;
    percent: number;
    speed: number;
    eta: string;
    status: "idle" | "checking" | "downloading" | "installing" | "waiting_auth" | "ready" | "error";
    version: string;
  }>({
    downloaded: 0,
    total: 0,
    percent: 0,
    speed: 0,
    eta: "---",
    status: "idle",
    version: ""
  });

  useEffect(() => {
    const savedVersion = localStorage.getItem("installed_app_version");
    if (savedVersion && savedVersion !== CURRENT_APP_VERSION) {
      setUpdatedFromVersion(savedVersion);
      setShowUpdateSuccessModal(true);
    }
    localStorage.setItem("installed_app_version", CURRENT_APP_VERSION);
  }, []);

  // Watcher effect: Relaunches app when ongoing downloads complete
  useEffect(() => {
    if (!pendingRelaunch) return;
    const hasActiveDownloads = downloads.some((d) => {
      const s = typeof d.status === "string" ? d.status : JSON.stringify(d.status);
      return s === "Downloading" || s === '"Downloading"';
    });

    if (!hasActiveDownloads) {
      setPendingRelaunch(false);
      setUpdateStatus("Active downloads completed! Relaunching application...");
      relaunch().catch(console.error);
    }
  }, [downloads, pendingRelaunch]);

  const triggerRelaunchOrWait = async () => {
    const hasActiveDownloads = downloads.some((d) => {
      const s = typeof d.status === "string" ? d.status : JSON.stringify(d.status);
      return s === "Downloading" || s === '"Downloading"';
    });

    if (hasActiveDownloads) {
      setPendingRelaunch(true);
      setUpdateStatus("Update installed! Application will restart automatically once active downloads finish.");
    } else {
      setUpdateStatus("Update installed! Relaunching application...");
      await relaunch();
    }
  };

  const executeUpdateInstallation = async (update: any) => {
    setIsDownloadingUpdate(true);
    setUpdateProgressInfo({
      downloaded: 0,
      total: 0,
      percent: 0,
      speed: 0,
      eta: "Calculating...",
      status: "downloading",
      version: update.version
    });
    setUpdateStatus(`Downloading update: v${update.version}...`);

    let downloaded = 0;
    let contentLength = 0;
    let lastTime = Date.now();
    let lastDownloaded = 0;

    try {
      await update.downloadAndInstall((event: any) => {
        if (!event) return;
        const now = Date.now();

        if (event.event === 'Started') {
          contentLength = event.data?.contentLength || 0;
          lastTime = now;
          lastDownloaded = 0;
          setUpdateProgressInfo((prev) => ({
            ...prev,
            total: contentLength,
            status: "downloading"
          }));
        } else if (event.event === 'Progress') {
          downloaded += event.data?.chunkLength || 0;
          const timeDiff = (now - lastTime) / 1000;
          let currentSpeed = 0;
          if (timeDiff >= 0.4) {
            const bytesDiff = downloaded - lastDownloaded;
            currentSpeed = bytesDiff / timeDiff;
            lastTime = now;
            lastDownloaded = downloaded;
          }

          const pct = contentLength > 0 ? Math.min(100, Math.floor((downloaded / contentLength) * 100)) : 0;
          const remainingBytes = contentLength > downloaded ? contentLength - downloaded : 0;
          const etaSecs = currentSpeed > 0 ? Math.ceil(remainingBytes / currentSpeed) : 0;
          const etaStr = etaSecs > 0 ? `${etaSecs}s` : "---";

          setUpdateProgressInfo((prev) => ({
            ...prev,
            downloaded,
            total: contentLength,
            percent: pct,
            speed: currentSpeed > 0 ? currentSpeed : prev.speed,
            eta: etaStr,
            status: "downloading"
          }));

          setUpdateStatus(`Downloading v${update.version}: ${formatBytes(downloaded)} / ${formatBytes(contentLength)} (${formatBytes(currentSpeed)}/s)`);
        } else if (event.event === 'Finished') {
          setUpdateProgressInfo((prev) => ({
            ...prev,
            percent: 100,
            status: "installing"
          }));
          setUpdateStatus("System installation starting. Please enter your Superuser (sudo) password in system prompt.");
        }
      });

      setIsDownloadingUpdate(false);
      setUpdateProgressInfo((prev) => ({
        ...prev,
        percent: 100,
        status: "ready"
      }));

      await triggerRelaunchOrWait();
    } catch (err: any) {
      setIsDownloadingUpdate(false);
      const errMsg = String(err?.message || err);
      setUpdateProgressInfo((prev) => ({
        ...prev,
        status: "error"
      }));
      setUpdateStatus(`Update check failed: ${errMsg}`);
    }
  };

  const handleCheckForUpdates = async () => {
    if (checkingUpdate || isDownloadingUpdate) return;
    setCheckingUpdate(true);
    setUpdateStatus("Checking for updates...");
    setUpdateProgressInfo((prev) => ({ ...prev, status: "checking" }));
    try {
      const update = await checkUpdate();
      if (update && update.available) {
        await executeUpdateInstallation(update);
      } else {
        setUpdateStatus("You are running the latest version!");
        setUpdateProgressInfo((prev) => ({ ...prev, status: "idle" }));
      }
    } catch (e: any) {
      console.error("Update check error:", e);
      setIsDownloadingUpdate(false);
      const errMsg = String(e?.message || e);
      if (errMsg.includes("Could not fetch a valid release JSON") || errMsg.includes("404")) {
        setUpdateStatus("You are running the latest version! No new update found.");
      } else {
        setUpdateStatus(`Update check error: ${errMsg}`);
      }
      setUpdateProgressInfo((prev) => ({ ...prev, status: "error" }));
    } finally {
      setCheckingUpdate(false);
    }
  };

  // Scheduler State
  const [schedulerEnabled, setSchedulerEnabled] = useState(false);
  const [startTime, setStartTime] = useState("02:00");
  const [endTime, setEndTime] = useState("06:00");
  const [activeDays, setActiveDays] = useState<string[]>(["Mon", "Tue", "Wed", "Thu", "Fri"]);

  const [isThemeDropdownOpen, setIsThemeDropdownOpen] = useState(false);
  const themeDropdownRef = useRef<HTMLDivElement>(null);
  const [activeTooltip, setActiveTooltip] = useState<{ title: string; x: number; y: number } | null>(null);

  const showTooltip = (title: string, e: React.MouseEvent) => {
    const rect = e.currentTarget.getBoundingClientRect();
    setActiveTooltip({
      title,
      x: rect.right + 12,
      y: rect.top + rect.height / 2
    });
  };

  const hideTooltip = () => setActiveTooltip(null);

  useEffect(() => {
    const handleOutsideClick = (e: MouseEvent) => {
      if (themeDropdownRef.current && !themeDropdownRef.current.contains(e.target as Node)) {
        setIsThemeDropdownOpen(false);
      }
    };
    document.addEventListener("mousedown", handleOutsideClick);
    return () => document.removeEventListener("mousedown", handleOutsideClick);
  }, []);

  // Theme Mode Settings State
  const [themeMode, setThemeMode] = useState<"dark" | "light" | "system">(() => {
    return (localStorage.getItem("theme_mode") as any) || "dark";
  });

  useEffect(() => {
    localStorage.setItem("theme_mode", themeMode);

    const applyTheme = () => {
      let resolvedTheme = "dark";
      if (themeMode === "system") {
        const isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        resolvedTheme = isDark ? "dark" : "light";
      } else {
        resolvedTheme = themeMode;
      }
      document.documentElement.setAttribute("data-theme", resolvedTheme);

      invoke("sync_theme_mode", { theme_mode: resolvedTheme }).catch(console.error);
    };

    applyTheme();

    if (themeMode === "system") {
      const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
      const handler = () => applyTheme();
      mediaQuery.addEventListener("change", handler);
      return () => mediaQuery.removeEventListener("change", handler);
    }
  }, [themeMode]);

  useEffect(() => {
    const handleStorageChange = (e: StorageEvent) => {
      if (e.key === "theme_mode") {
        setThemeMode((e.newValue as any) || "dark");
      }
    };
    window.addEventListener("storage", handleStorageChange);
    return () => window.removeEventListener("storage", handleStorageChange);
  }, []);

  const daysOfWeek = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

  const toggleDay = (day: string) => {
    if (activeDays.includes(day)) {
      setActiveDays(activeDays.filter((d) => d !== day));
    } else {
      setActiveDays([...activeDays, day]);
    }
  };

  // Parse filename from URL
  const extractFilename = (url: string): string => {
    try {
      const parsed = new URL(url);
      const pathname = parsed.pathname;
      const lastSegment = pathname.substring(pathname.lastIndexOf("/") + 1);
      return lastSegment || "downloaded_file";
    } catch {
      return "downloaded_file";
    }
  };

  // Triggered manually
  const handleOpenAddModal = async () => {
    setShowAddModal(true);
    try {
      const text = await navigator.clipboard.readText();
      if (text.startsWith("http://") || text.startsWith("https://")) {
        setInputUrl(text);
        setCustomFilename(extractFilename(text));
      }
    } catch (e) {
      console.warn("Clipboard reading failed:", e);
    }
  };

  // Auto-fill filename when URL input changes
  const handleUrlChange = (val: string) => {
    setInputUrl(val);
    if (val.startsWith("http://") || val.startsWith("https://")) {
      setCustomFilename(extractFilename(val));
    }
  };

  // Diagnostic Error Banner State
  const [initError, setInitError] = useState<string | null>(null);
  const [isStartingDownload, setIsStartingDownload] = useState(false);

  // Initialization query-param routing & initial loading
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const mode = params.get("popup");
    const url = params.get("url") || "";
    const filename = params.get("filename") || "";
    const savePath = params.get("save_path") || "";
    const cookie = params.get("cookie") || "";
    const referrer = params.get("referrer") || "";
    const size = params.get("size") || "0";
    const taskId = params.get("id") || null;

    // Fetch OS default download directory if no custom saved path exists
    const savedDir = localStorage.getItem("default_save_dir");
    if (!savedDir) {
      invoke<string>("get_default_download_dir")
        .then((dir) => {
          if (dir) {
            setDefaultSaveDir(dir);
            setSavePath(dir);
          }
        })
        .catch((err) => {
          console.error("get_default_download_dir error:", err);
          setInitError(`Failed to get default download directory: ${err?.message || err}`);
        });
    } else {
      setDefaultSaveDir(savedDir);
      setSavePath(savedDir);
    }

    // Sync autostart settings to OS registry / desktop entry
    const savedAutostart = localStorage.getItem("autostart");
    const isAutostartEnabled = savedAutostart !== null ? savedAutostart === "true" : true;
    invoke("toggle_autostart", { enabled: isAutostartEnabled }).catch(console.error);

    // Sync speed limit settings to backend engine
    const isSpeedLimitEnabled = localStorage.getItem("speed_limit_enabled") === "true";
    const savedVal = localStorage.getItem("speed_limit_val") || "512";
    const savedUnit = localStorage.getItem("speed_limit_unit") || "KB";
    const limitBps = isSpeedLimitEnabled ? calculateLimitBps(savedVal, savedUnit) : 0;
    invoke("set_speed_limit", { limitBps }).catch(console.error);

    // Sync intercept downloads setting to backend engine
    const savedIntercept = localStorage.getItem("intercept_downloads");
    const isInterceptEnabled = savedIntercept !== null ? savedIntercept === "true" : true;
    invoke("set_intercept_downloads", { enabled: isInterceptEnabled }).catch(console.error);

    // Sync max segment connections setting to backend engine
    const savedMaxChunks = localStorage.getItem("max_chunks") || "8";
    const maxChunksVal = Math.min(32, Math.max(1, parseInt(savedMaxChunks, 10) || 8));
    invoke("set_max_chunks", { maxChunks: maxChunksVal }).catch(console.error);

    if (mode) {
      setPopupMode(mode);
      setPopupUrl(url);
      setPopupFilename(decodeURIComponent(filename));
      if (savePath) {
        setSavePath(decodeURIComponent(savePath));
      }
      setPopupSavePath(decodeURIComponent(savePath));
      setPopupCookie(cookie);
      setPopupReferrer(referrer);
      setPopupTaskId(taskId);
      setPopupSize(size);

      // Fetch progress state immediately from Rust backend to prevent Connecting... freeze
      if (mode === "progress" && taskId) {
        invoke<DownloadProgress | null>("get_download_progress", { id: taskId })
          .then((prog) => {
            if (prog) setPopupProgress(prog);
          })
          .catch((err) => {
            console.error("get_download_progress error:", err);
            setInitError(`Failed to get download progress (ID: ${taskId}): ${err?.message || err}`);
          });
      } else if (mode === "complete") {
        setPopupFilename(params.get("filename") || "");
      }
    } else {
      // Main dashboard: fetch all active/completed downloads from backend
      invoke<DownloadProgress[]>("get_all_downloads")
        .then((list) => {
          if (list) setDownloads(list);
        })
        .catch((err) => {
          console.error("get_all_downloads error:", err);
          setInitError(`IPC Connection Error (get_all_downloads): ${err?.message || err}`);
        });

      // Background silent update check on application startup
      checkUpdate()
        .then(async (update: any) => {
          if (update && update.available) {
            console.log(`[Auto-Update] New version ${update.version} found! Downloading in background...`);
            await executeUpdateInstallation(update);
          }
        })
        .catch((err: any) => {
          setIsDownloadingUpdate(false);
          // Silently handle background update check failures (e.g. offline or no release yet)
          console.log("[Auto-Update] Startup check:", err?.message || err);
        });
    }
  }, []);

  // Handler for setting default download directory in settings
  const handleUpdateDefaultSaveDir = (newDir: string) => {
    setDefaultSaveDir(newDir);
    setSavePath(newDir);
    localStorage.setItem("default_save_dir", newDir);
  };

  const handlePickDefaultFolder = async () => {
    try {
      const chosen = await invoke<string>("select_folder");
      if (chosen) {
        handleUpdateDefaultSaveDir(chosen);
      }
    } catch (e) {
      console.warn("Folder picker cancelled or failed:", e);
    }
  };



  // Poll to keep main dashboard synced with background downloads
  useEffect(() => {
    if (popupMode) return;

    const interval = setInterval(async () => {
      try {
        const list = await invoke<DownloadProgress[]>("get_all_downloads");
        if (list) setDownloads(list);
      } catch (e) {
        console.error("Failed to sync downloads:", e);
      }
    }, 2000);

    return () => clearInterval(interval);
  }, [popupMode]);

  // Polling loop for the progress popup window (200ms interval)
  useEffect(() => {
    if (popupMode !== "progress" || !popupTaskId) return;

    const id = popupTaskId;
    let stopped = false;

    const poll = async () => {
      while (!stopped) {
        try {
          const prog = await invoke<DownloadProgress | null>("get_download_progress", { id });
          if (prog) {
            setPopupProgress(prog);
            if (prog.status === "Completed") {
              stopped = true;
              await invoke("open_complete_window", { filename: prog.filename, savePath: prog.save_path || "" });
              await invoke("close_window");
              return;
            }
          }
        } catch (e) {
          console.error("Progress poll error:", e);
        }
        await new Promise<void>((res) => setTimeout(res, 200));
      }
    };

    poll();
    return () => { stopped = true; };
  }, [popupMode, popupTaskId]);

  // Event listener: only handles main-dashboard updates + browser intercept
  useEffect(() => {
    let unlistenProgress: (() => void) | undefined;
    let unlistenIntercept: (() => void) | undefined;

    async function setupListeners() {
      // Live progress events — only used by the main dashboard now
      // (progress popup uses its own polling loop instead)
      unlistenProgress = await listen<DownloadProgress>("download-progress", (event) => {
        if (popupMode === "progress") return; // Popup handles itself via poll

        setDownloads((prev) => {
          const index = prev.findIndex((d) => d.id === event.payload.id);
          const updated = [...prev];
          if (index !== -1) {
            updated[index] = { ...prev[index], ...event.payload };
            return updated;
          } else {
            return [event.payload, ...prev];
          }
        });
      });

      // Browser automatic interception — only on main dashboard window
      if (!popupMode) {
        unlistenIntercept = await listen<{ url: string; filename: string; cookie?: string; referrer?: string }>("download-intercepted", (event) => {
          setInputUrl(event.payload.url);
          setCustomFilename(event.payload.filename);
          setPopupCookie(event.payload.cookie || "");
          setPopupReferrer(event.payload.referrer || "");
          setShowAddModal(true);
        });
      }
    }

    setupListeners().catch(console.error);

    return () => {
      if (unlistenProgress) unlistenProgress();
      if (unlistenIntercept) unlistenIntercept();
    };
  }, [popupMode]);

  // Submit start download from main window modal
  const handleStartDownload = async () => {
    if (!inputUrl || isStartingDownload) return;
    setIsStartingDownload(true);
    const finalFilename = customFilename || extractFilename(inputUrl);
    const finalSavePath = `${savePath.endsWith("/") ? savePath : savePath + "/"}${finalFilename}`;

    try {
      const id = await invoke<string>("start_download", {
        url: inputUrl,
        filename: finalFilename,
        savePath: finalSavePath,
        cookie: popupCookie || "",
        referrer: popupReferrer || ""
      });

      const newTask: DownloadProgress = {
        id,
        filename: finalFilename,
        total_size: 0,
        downloaded: 0,
        speed: 0,
        eta: "---",
        status: "Queued",
        url: inputUrl,
        save_path: finalSavePath,
      };

      setDownloads((prev) => [newTask, ...prev]);
      setShowAddModal(false);
      setInputUrl("");
      setCustomFilename("");
      setIsStartingDownload(false);

      // Open standalone progress window for this task
      await invoke("open_progress_window", { id });
    } catch (e) {
      console.error("Failed to start download:", e);
      setIsStartingDownload(false);
    }
  };

  // Submit start download from Popup 1 (Standalone Add window)
  const handlePopupStartDownload = async () => {
    if (!popupUrl || isStartingDownload) return;
    setIsStartingDownload(true);
    const finalFilename = popupFilename || extractFilename(popupUrl);
    const finalSavePath = `${savePath.endsWith("/") ? savePath : savePath + "/"}${finalFilename}`;

    try {
      const id = await invoke<string>("start_download", {
        url: popupUrl,
        filename: finalFilename,
        savePath: finalSavePath,
        cookie: popupCookie || "",
        referrer: popupReferrer || ""
      });

      // Open standalone Progress window (Popup 2), which closes this Add window
      await invoke("open_progress_window", { id });
    } catch (e) {
      console.error("Failed to start popup download:", e);
      setIsStartingDownload(false);
    }
  };

  // Trigger system folder picker GUI
  const handlePickFolder = async () => {
    try {
      const chosen = await invoke<string>("select_folder");
      setSavePath(chosen);
    } catch (e) {
      console.warn("Folder picker cancelled or failed:", e);
    }
  };

  // Controls bindings
  const handlePause = async (e: React.MouseEvent | null, id: string) => {
    if (e) e.stopPropagation();
    try {
      await invoke("pause_download", { id });
    } catch (err) {
      console.error(err);
    }
  };

  const handleResume = async (e: React.MouseEvent | null, id: string) => {
    if (e) e.stopPropagation();
    try {
      if (popupMode === "progress") {
        // Inside a progress popup: just resume, the 500ms poll will update UI
        await invoke("resume_download", { id });
      } else {
        // In main dashboard: resume AND open/focus the progress popup
        await invoke("resume_and_open_progress", { id });
      }
    } catch (err) {
      console.error(err);
    }
  };

  const handleRedownload = async (e: React.MouseEvent | null, id: string) => {
    if (e) e.stopPropagation();
    try {
      if (popupMode === "progress") {
        await invoke("redownload_task", { id });
      } else {
        await invoke("redownload_and_open_progress", { id });
      }
    } catch (err) {
      console.error(err);
    }
  };

  const handleRefreshLink = async (e: React.MouseEvent | null, id: string) => {
    if (e) e.stopPropagation();
    try {
      await invoke("refresh_download_link", { id });
    } catch (err) {
      console.error(err);
    }
  };

  const handleCancel = async (e: React.MouseEvent | null, id: String) => {
    if (e) e.stopPropagation();
    try {
      await invoke("cancel_download", { id });

      // If we are cancel-closing the standalone progress or refresh window, close this window
      if (popupMode === "progress" || popupMode === "refresh") {
        await handleClosePopup();
      }
    } catch (err) {
      console.error(err);
    }
  };

  const handleOpenFileDir = async (e: React.MouseEvent | null, path: string) => {
    if (e) e.stopPropagation();
    try {
      await invoke("open_file_dir", { path });
    } catch (err) {
      console.error(err);
    }
  };

  const promptRemoveTask = (e: React.MouseEvent | null, task: DownloadProgress) => {
    if (e) e.stopPropagation();
    setTaskToRemove(task);
    setDeleteFileFromDisk(false);
    setShowRemoveConfirm(true);
  };

  const confirmRemoveTask = async () => {
    if (!taskToRemove) return;
    const id = taskToRemove.id;
    const isAlreadyTrash = getStatusText(taskToRemove.status) === "Trash";

    try {
      if (isAlreadyTrash) {
        await invoke("delete_task", { id });
        setDownloads((prev) => prev.filter((d) => d.id !== id));
      } else {
        await invoke("trash_task", { id, deleteFile: deleteFileFromDisk });
        setDownloads((prev) =>
          prev.map((d) => d.id === id ? { ...d, status: "Trash" } : d)
        );
      }
      if (selectedTask?.id === id) setSelectedTask(null);
    } catch (err) {
      console.error(err);
    } finally {
      setShowRemoveConfirm(false);
      setTaskToRemove(null);
    }
  };




  const handleClosePopup = async () => {
    try {
      await invoke("close_window");
    } catch (e) {
      try {
        await getCurrentWindow().close();
      } catch (err) {
        console.error("Failed to close window:", err);
      }
    }
  };

  // Helpers
  const getFileIcon = (filename: string) => {
    const ext = filename.split(".").pop()?.toLowerCase();
    if (!ext) return <File size={22} />;
    if (["mp4", "mkv", "avi", "mov", "flv", "webm"].includes(ext)) return <Film size={22} />;
    if (["mp3", "wav", "aac", "flac", "m4a", "ogg"].includes(ext)) return <Music size={22} />;
    if (["pdf", "docx", "doc", "txt", "xlsx", "pptx", "epub"].includes(ext)) return <FileText size={22} />;
    if (["zip", "rar", "7z", "tar", "gz", "bz2"].includes(ext)) return <Archive size={22} />;
    return <File size={22} />;
  };

  const getFileCategory = (filename: string): string => {
    const ext = filename.split(".").pop()?.toLowerCase();
    if (!ext) return "other";
    if (["mp4", "mkv", "avi", "mov", "flv", "webm"].includes(ext)) return "videos";
    if (["mp3", "wav", "aac", "flac", "m4a", "ogg"].includes(ext)) return "audio";
    if (["pdf", "docx", "doc", "txt", "xlsx", "pptx", "epub"].includes(ext)) return "documents";
    if (["zip", "rar", "7z", "tar", "gz", "bz2"].includes(ext)) return "archives";
    if (["exe", "msi", "deb", "rpm", "dmg", "pkg", "appimage", "apk"].includes(ext)) return "executables";
    return "other";
  };

  const getStatusText = (status: DownloadStatus): string => {
    if (typeof status === "string") return status;
    if (status && typeof status === "object" && "Failed" in status) {
      return `Failed: ${status.Failed}`;
    }
    return "Unknown";
  };

  const filteredDownloads = downloads.filter((d) => {
    if (searchQuery && !d.filename.toLowerCase().includes(searchQuery.toLowerCase())) return false;
    const statusText = getStatusText(d.status);
    if (activeCategory === "trash") return statusText === "Trash";
    if (statusText === "Trash") return false; // Exclude from all other categories

    if (activeCategory === "all") return true;
    if (activeCategory === "downloading") return statusText === "Downloading" || statusText === "Queued";
    if (activeCategory === "completed") return statusText === "Completed";
    if (activeCategory === "paused") return statusText === "Paused";
    return getFileCategory(d.filename) === activeCategory;
  });

  const sortedDownloads = [...filteredDownloads].sort((a, b) => {
    const statusA = getStatusText(a.status);
    const statusB = getStatusText(b.status);

    const isAActive = statusA === "Downloading" || statusA === "Queued" || statusA === "Paused";
    const isBActive = statusB === "Downloading" || statusB === "Queued" || statusB === "Paused";

    if (isAActive && !isBActive) return -1;
    if (!isAActive && isBActive) return 1;
    return 0;
  });



  const mainCategories: Category[] = [
    { id: "all", name: "All Downloads", icon: <Layers size={18} /> },
    { id: "downloading", name: "Downloading", icon: <Activity size={18} /> },
    { id: "completed", name: "Completed", icon: <CheckCircle2 size={18} /> },
    { id: "paused", name: "Paused", icon: <Clock size={18} /> },
    { id: "trash", name: "Trash", icon: <Trash2 size={18} /> },
  ];

  const fileCategories: Category[] = [
    { id: "videos", name: "Videos", icon: <Film size={16} /> },
    { id: "audio", name: "Audio", icon: <Music size={16} /> },
    { id: "documents", name: "Documents", icon: <FileText size={16} /> },
    { id: "archives", name: "Archives", icon: <Archive size={16} /> },
  ];

  // STANDALONE POPUP RENDER logic
  if (popupMode === "add") {
    return (
      <div className="modal-content-v2" style={{ height: "100vh", overflowY: "auto", animation: "none", boxShadow: "none", border: "none", padding: "16px" }}>

        <div className="modal-body-v2" style={{ display: "flex", flexDirection: "column", gap: "10px", padding: 0 }}>
          <div className="form-group-v2">
            <span className="form-label-v2" style={{ fontSize: "0.75rem", marginBottom: "4px" }}>Source URL</span>
            <input type="text" className="spotlight-input" style={{ padding: "8px 12px", fontSize: "0.85rem" }} value={popupUrl} onChange={(e) => setPopupUrl(e.target.value)} />
          </div>
          <div className="form-group-v2">
            <span className="form-label-v2" style={{ fontSize: "0.75rem", marginBottom: "4px" }}>Save As Filename</span>
            <input type="text" className="spotlight-input" style={{ padding: "8px 12px", fontSize: "0.85rem" }} value={popupFilename} onChange={(e) => setPopupFilename(e.target.value)} />
          </div>
          <div className="form-group-v2">
            <span className="form-label-v2" style={{ fontSize: "0.75rem", marginBottom: "4px" }}>Save Folder Path</span>
            <div style={{ display: "flex", gap: "8px" }}>
              <input type="text" className="spotlight-input" style={{ padding: "8px 12px", fontSize: "0.85rem", flex: 1 }} value={savePath} onChange={(e) => setSavePath(e.target.value)} />
              <button className="hover-action-btn" style={{ width: "auto", padding: "0 12px", fontSize: "0.8rem", height: "34px" }} onClick={handlePickFolder}>Browse</button>
            </div>
          </div>
          <div className="form-group-v2" style={{ marginTop: "2px" }}>
            <span style={{ fontSize: "0.85rem", color: "var(--text-secondary)" }}>
              Estimated File Size: <span style={{ fontWeight: 700, color: "var(--accent-cyan)" }}>{popupSize && popupSize !== "0" ? formatBytes(parseInt(popupSize)) : "Unknown Size"}</span>
            </span>
          </div>
        </div>

        <div className="modal-actions-v2" style={{ marginTop: "12px", paddingTop: "8px" }}>
          <button className="hover-action-btn" style={{ width: "auto", padding: "0 16px", height: "38px", fontSize: "0.9rem" }} onClick={handleClosePopup} disabled={isStartingDownload}>Cancel</button>
          <button className="accent-pill" style={{ padding: "8px 24px", borderRadius: "100px", fontWeight: 700, fontSize: "0.9rem", height: "38px", opacity: isStartingDownload ? 0.7 : 1 }} onClick={handlePopupStartDownload} disabled={!popupUrl || isStartingDownload}>
            {isStartingDownload ? "Starting..." : "Start Download"}
          </button>
        </div>
      </div>
    );
  }

  if (popupMode === "progress") {
    const isCompleted = popupProgress?.status === "Completed";
    const progressPercent = isCompleted
      ? 100
      : (popupProgress && popupProgress.total_size > 0
        ? Math.min(100, Math.floor((popupProgress.downloaded / popupProgress.total_size) * 100))
        : 0);
    const isPaused = popupProgress && (
      popupProgress.status === "Paused" ||
      JSON.stringify(popupProgress.status) === JSON.stringify("Paused")
    );
    const catClass = popupProgress ? `cat-${getFileCategory(popupProgress.filename)}` : "";

    return (
      <div className="modal-content-v2" style={{ height: "100vh", animation: "none", boxShadow: "none", border: "none", alignItems: "center", justifyContent: "center", position: "relative" }}>

        <div className="liquid-progress-container" style={{ width: "100px", height: "100px", marginBottom: "16px" }}>
          <div
            className={`liquid-fill ${isPaused ? "paused" : ""} ${isCompleted ? "completed" : ""} ${catClass}`}
            style={{ transform: `translateY(${100 - progressPercent}%)` }}
          />
          <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", zIndex: 10 }}>
            <span style={{ fontSize: "1.8rem", fontWeight: 800, color: "#fff", textShadow: "0 1px 3px rgba(0,0,0,0.9), 0 0 2px rgba(0,0,0,0.9)", lineHeight: 1 }}>{progressPercent}%</span>
            <span style={{ fontSize: "0.7rem", color: "rgba(255,255,255,0.9)", fontWeight: 600, marginTop: "2px", textShadow: "0 1px 3px rgba(0,0,0,0.9), 0 0 2px rgba(0,0,0,0.9)" }}>{popupProgress ? getStatusText(popupProgress.status) : "Connecting"}</span>
          </div>
        </div>

        <div style={{ textAlign: "center", width: "90%" }}>
          <div className="file-display-box" style={{ background: "rgba(255,255,255,0.05)", border: "none", fontSize: "0.85rem", padding: "8px 12px", marginBottom: "12px", borderRadius: "8px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={popupProgress?.filename}>
            {popupProgress?.filename || "Loading..."}
          </div>

          {popupProgress && (
            <div style={{ display: "flex", justifyContent: "space-between", background: "rgba(0,0,0,0.2)", borderRadius: "12px", padding: "10px", border: "1px solid rgba(255,255,255,0.03)" }}>
              <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
                <span style={{ fontSize: "0.65rem", color: "var(--text-secondary)", textTransform: "uppercase", letterSpacing: "0.5px" }}>Downloaded</span>
                <span style={{ fontSize: "0.85rem", fontWeight: 700 }}>
                  {formatBytes(popupProgress.downloaded)}
                  <span style={{ fontSize: "0.65rem", color: "var(--text-secondary)", fontWeight: 500, marginLeft: "4px" }}>
                    / {popupProgress.total_size > 0 ? formatBytes(popupProgress.total_size) : "???"}
                  </span>
                </span>
              </div>
              <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
                <span style={{ fontSize: "0.65rem", color: "var(--text-secondary)", textTransform: "uppercase", letterSpacing: "0.5px" }}>
                  Speed {popupProgress.speed_limited && <span style={{ color: "var(--accent-orange)", fontSize: "0.6rem", background: "rgba(245, 158, 11, 0.15)", padding: "1px 4px", borderRadius: "4px", marginLeft: "4px", fontWeight: 700 }}>LIMITED</span>}
                </span>
                <span className="accent-cyan" style={{ fontSize: "0.85rem", fontWeight: 700 }}>{formatBytes(popupProgress.speed)}/s</span>
              </div>
              <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
                <span style={{ fontSize: "0.65rem", color: "var(--text-secondary)", textTransform: "uppercase", letterSpacing: "0.5px" }}>ETA</span>
                <span style={{ fontSize: "0.85rem", fontWeight: 700 }}>{popupProgress.eta}</span>
              </div>
            </div>
          )}
        </div>

        <div style={{ position: "absolute", bottom: "16px", left: "16px", right: "16px", display: "flex", gap: "8px", justifyContent: "center" }}>
          {popupProgress && (
            <>
              {isPaused ? (
                <button className="hover-action-btn" style={{ flex: 1, padding: "14px 0", borderRadius: "16px" }} onClick={() => handleResume(null, popupProgress.id)}>
                  <Play size={18} style={{ marginRight: "8px" }} /> Resume
                </button>
              ) : (
                <button className="hover-action-btn" style={{ flex: 1, padding: "14px 0", borderRadius: "16px" }} onClick={() => handlePause(null, popupProgress.id)}>
                  <Pause size={18} style={{ marginRight: "8px" }} /> Pause
                </button>
              )}
              <button className="hover-action-btn" style={{ flex: 1, padding: "14px 0", borderRadius: "16px", color: "var(--accent-red)" }} onClick={() => handleCancel(null, popupProgress.id)}>
                <X size={18} style={{ marginRight: "8px" }} /> Cancel
              </button>
            </>
          )}
        </div>
      </div>
    );
  }

  if (popupMode === "refresh") {
    const pId = new URLSearchParams(window.location.search).get("id") || "";
    return (
      <div className="modal-content-v2" style={{ height: "100vh", animation: "none", boxShadow: "none", border: "none", alignItems: "center", justifyContent: "center", position: "relative" }}>
        <RefreshCw size={56} className="spin-slow" color="var(--accent-orange)" style={{ marginBottom: "24px" }} />
        <div style={{ color: "var(--accent-orange)", fontSize: "1.6rem", fontWeight: 800, marginBottom: "12px" }}>
          Waiting for Capture
        </div>
        <p style={{ fontSize: "1rem", color: "var(--text-secondary)", textAlign: "center", width: "85%", lineHeight: 1.6 }}>
          We opened your web browser to the download page. Simply click the download button in your browser now. We will capture the updated address and resume automatically!
        </p>
        <button className="hover-action-btn" style={{ position: "absolute", bottom: "16px", width: "calc(100% - 48px)", padding: "12px", borderRadius: "16px", color: "var(--accent-red)", fontWeight: 700, fontSize: "0.85rem" }} onClick={() => handleCancel(null, pId)}>
          Cancel Refresh
        </button>
      </div>
    );
  }

  if (popupMode === "complete") {
    return (
      <div className="modal-content-v2" style={{ height: "100vh", animation: "none", boxShadow: "none", border: "none", alignItems: "center", justifyContent: "center", position: "relative", padding: "24px" }}>
        <div style={{ width: "80px", height: "80px", borderRadius: "50%", background: "rgba(16, 185, 129, 0.1)", display: "flex", alignItems: "center", justifyContent: "center", marginBottom: "16px", boxShadow: "0 0 30px rgba(16, 185, 129, 0.15)" }}>
          <CheckCircle size={40} color="var(--accent-green)" />
        </div>
        <div style={{ fontSize: "1.6rem", fontWeight: 800, color: "var(--text-primary)", marginBottom: "12px", letterSpacing: "-0.5px" }}>
          Download Complete!
        </div>
        <div className="file-display-box" style={{ background: "rgba(255,255,255,0.03)", border: "1px solid rgba(255,255,255,0.06)", fontSize: "0.95rem", padding: "12px 20px", width: "90%", textAlign: "center", marginBottom: "32px", borderRadius: "12px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={popupFilename}>
          {popupFilename}
        </div>

        <div style={{ position: "absolute", bottom: "24px", left: "24px", right: "24px", display: "flex", gap: "12px" }}>
          {popupSavePath && (
            <button
              className="hover-action-btn"
              style={{ flex: 1, padding: "14px", borderRadius: "12px", color: "var(--accent-green)", fontWeight: 700, fontSize: "0.95rem", background: "rgba(16, 185, 129, 0.05)", border: "1px solid rgba(16, 185, 129, 0.15)" }}
              onClick={() => handleOpenFileDir(null, popupSavePath)}
            >
              Open Folder
            </button>
          )}
          <button
            className="hover-action-btn"
            style={{ flex: 1, padding: "14px", borderRadius: "12px", fontWeight: 700, fontSize: "0.95rem" }}
            onClick={handleClosePopup}
          >
            Close
          </button>
        </div>
      </div>
    );
  }

  // DEFAULT MAIN DASHBOARD view
  return (
    <div className="app-shell v2-shell">
      {/* Sidebar V2 */}
      <aside className="sidebar-v2">
        <div className="brand-v2">
          <img src="/logo.png" className="brand-logo-img-v2" alt="Logo" />
        </div>

        <nav className="sidebar-menu-v2">
          <button
            className="menu-pill accent-pill"
            onClick={handleOpenAddModal}
            onMouseEnter={(e) => showTooltip("Add Download", e)}
            onMouseLeave={hideTooltip}
          >
            <Plus size={22} strokeWidth={3} />
          </button>

          <div className="sidebar-divider" />

          {mainCategories.map((cat) => (
            <div
              key={cat.id}
              className={`menu-pill ${activeCategory === cat.id ? "active" : ""}`}
              onClick={() => setActiveCategory(cat.id)}
              onMouseEnter={(e) => showTooltip(cat.name, e)}
              onMouseLeave={hideTooltip}
            >
              {cat.icon}
            </div>
          ))}

          <div className="sidebar-divider" />

          {fileCategories.map((cat) => (
            <div
              key={cat.id}
              className={`menu-pill ${activeCategory === cat.id ? "active" : ""}`}
              onClick={() => setActiveCategory(cat.id)}
              onMouseEnter={(e) => showTooltip(cat.name, e)}
              onMouseLeave={hideTooltip}
            >
              {cat.icon}
            </div>
          ))}

          <div style={{ marginTop: 'auto' }} />

          <div
            className={`menu-pill ${activeCategory === "settings" ? "active" : ""}`}
            onClick={() => setActiveCategory("settings")}
            onMouseEnter={(e) => showTooltip("Settings", e)}
            onMouseLeave={hideTooltip}
          >
            <Settings size={20} />
          </div>
        </nav>
      </aside>

      {/* Main Container */}
      <div className="main-canvas">
        {initError && (
          <div style={{
            background: "rgba(239, 68, 68, 0.15)",
            border: "1px solid #ef4444",
            color: "#fca5a5",
            padding: "12px 16px",
            margin: "16px 16px 0 16px",
            borderRadius: "10px",
            fontSize: "0.85rem",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            boxShadow: "0 4px 12px rgba(239, 68, 68, 0.2)"
          }}>
            <div>
              <strong style={{ color: "#ef4444" }}>⚠️ Diagnostic Alert:</strong> {initError}
            </div>
            <button
              onClick={() => setInitError(null)}
              style={{
                background: "none",
                border: "none",
                color: "#fca5a5",
                cursor: "pointer",
                padding: "4px 8px",
                fontSize: "1rem"
              }}
            >
              ✕
            </button>
          </div>
        )}

        {/* Spotlight Search Header */}
        <header className="topbar-v2">
          {activeCategory !== "settings" ? (
            <div className="spotlight-search-container">
              <Search size={18} className="spotlight-icon" />
              <input
                type="text"
                placeholder="Search downloads..."
                className="spotlight-input"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
            </div>
          ) : (
            <div style={{ fontSize: "1rem", fontWeight: 600, color: "var(--text-primary)" }}>
              Settings
            </div>
          )}
        </header>

        {/* Content Area */}
        <main className="content-area-v2">
          {activeCategory === "settings" ? (
            <div className="settings-container">
              <div className="settings-card">
                <div className="settings-section-header">
                  <Sliders size={18} />
                  <span className="settings-section-title">General Settings</span>
                </div>
                <div className="form-group">
                  <span className="form-label">Default Downloads Directory</span>
                  <div style={{ display: "flex", gap: "10px", alignItems: "center" }}>
                    <input
                      type="text"
                      className="form-input"
                      style={{ flex: 1 }}
                      value={defaultSaveDir}
                      onChange={(e) => handleUpdateDefaultSaveDir(e.target.value)}
                    />
                    <button
                      type="button"
                      className="hover-action-btn"
                      style={{ width: "auto", padding: "0 18px", height: "42px", flexShrink: 0 }}
                      onClick={handlePickDefaultFolder}
                    >
                      Browse
                    </button>
                  </div>
                </div>
                <div className="form-group" style={{ position: "relative" }}>
                  <span className="form-label">Theme Mode</span>
                  <div className="custom-dropdown-container" ref={themeDropdownRef}>
                    <button
                      type="button"
                      className="form-input custom-dropdown-trigger"
                      onClick={() => setIsThemeDropdownOpen(!isThemeDropdownOpen)}
                    >
                      {themeMode === "dark" && "Dark Theme"}
                      {themeMode === "light" && "Light Theme"}
                      {themeMode === "system" && "Follow System Theme"}
                    </button>
                    {isThemeDropdownOpen && (
                      <div className="custom-dropdown-menu">
                        <div
                          className={`custom-dropdown-item ${themeMode === "dark" ? "selected" : ""}`}
                          onClick={() => {
                            setThemeMode("dark");
                            setIsThemeDropdownOpen(false);
                          }}
                        >
                          Dark Theme
                        </div>
                        <div
                          className={`custom-dropdown-item ${themeMode === "light" ? "selected" : ""}`}
                          onClick={() => {
                            setThemeMode("light");
                            setIsThemeDropdownOpen(false);
                          }}
                        >
                          Light Theme
                        </div>
                        <div
                          className={`custom-dropdown-item ${themeMode === "system" ? "selected" : ""}`}
                          onClick={() => {
                            setThemeMode("system");
                            setIsThemeDropdownOpen(false);
                          }}
                        >
                          Follow System Theme
                        </div>
                      </div>
                    )}
                  </div>
                </div>
                <div className="settings-control-row">
                  <div className="settings-info-col">
                    <span className="settings-title">Launch on Startup</span>
                    <span className="settings-desc">Automatically launch Impressive Download Manager when your computer starts.</span>
                  </div>
                  <label className="switch-container">
                    <input
                      type="checkbox"
                      className="switch-input"
                      checked={autostart}
                      onChange={(e) => {
                        const val = e.target.checked;
                        setAutostart(val);
                        localStorage.setItem("autostart", String(val));
                        invoke("toggle_autostart", { enabled: val }).catch(console.error);
                      }}
                    />
                    <span className="switch-slider"></span>
                  </label>
                </div>
                <div className="settings-control-row">
                  <div className="settings-info-col">
                    <span className="settings-title">Minimize to System Tray</span>
                    <span className="settings-desc">Close button hides the window to system tray instead of exiting the process.</span>
                  </div>
                  <label className="switch-container">
                    <input
                      type="checkbox"
                      className="switch-input"
                      checked={minimizeToTray}
                      onChange={(e) => setMinimizeToTray(e.target.checked)}
                    />
                    <span className="switch-slider"></span>
                  </label>
                </div>
              </div>

              <div className="settings-card">
                <div className="settings-section-header">
                  <Network size={18} />
                  <span className="settings-section-title">Network & Performance</span>
                </div>
                <div className="settings-control-row" style={{ alignItems: "flex-start", flexDirection: "column", gap: "10px" }}>
                  <div className="settings-info-col">
                    <span className="settings-title">Max Segment Connections ({maxChunks})</span>
                    <span className="settings-desc">The maximum number of parallel range threads to split download files into in Rust.</span>
                  </div>
                  <input
                    type="range"
                    min="1"
                    max="32"
                    className="range-slider"
                    value={maxChunks}
                    onChange={(e) => {
                      const val = parseInt(e.target.value, 10);
                      setMaxChunks(val);
                      localStorage.setItem("max_chunks", String(val));
                      invoke("set_max_chunks", { maxChunks: val }).catch(console.error);
                    }}
                  />
                </div>
                <div className="settings-control-row">
                  <div className="settings-info-col">
                    <span className="settings-title">Apply Speed Limit</span>
                    <span className="settings-desc">Prevent downloads from consuming the entire network bandwidth.</span>
                  </div>
                  <label className="switch-container">
                    <input
                      type="checkbox"
                      className="switch-input"
                      checked={speedLimitEnabled}
                      onChange={(e) => {
                        const enabled = e.target.checked;
                        setSpeedLimitEnabled(enabled);
                        localStorage.setItem("speed_limit_enabled", String(enabled));
                        const curVal = speedLimitVal || "512";
                        const curUnit = speedLimitUnit || "KB";
                        if (!speedLimitVal) {
                          setSpeedLimitVal("512");
                          localStorage.setItem("speed_limit_val", "512");
                        }
                        const limitBps = enabled ? calculateLimitBps(curVal, curUnit) : 0;
                        invoke("set_speed_limit", { limitBps }).catch(console.error);
                      }}
                    />
                    <span className="switch-slider"></span>
                  </label>
                </div>
                {speedLimitEnabled && (
                  <div className="settings-control-row" style={{ alignItems: "flex-start", flexDirection: "column", gap: "12px", background: "rgba(0,0,0,0.15)", padding: "16px", borderRadius: "12px", border: "1px solid rgba(255,255,255,0.04)" }}>
                    <div style={{ display: "flex", justifyContent: "space-between", width: "100%", alignItems: "center" }}>
                      <span className="settings-title">Maximum Speed Threshold</span>
                      <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                        <input
                          type="text"
                          className="spotlight-input"
                          style={{
                            width: "90px",
                            padding: "6px 10px",
                            textAlign: "right",
                            fontSize: "0.9rem",
                            fontWeight: 700,
                            color: "var(--accent-cyan)",
                            background: themeMode === "light" ? "#ffffff" : "rgba(0, 0, 0, 0.25)",
                            borderRadius: "8px",
                            border: themeMode === "light" ? "1px solid rgba(0, 0, 0, 0.12)" : "1px solid rgba(255, 255, 255, 0.08)"
                          }}
                          value={speedLimitVal}
                          placeholder="512"
                          onChange={(e) => {
                            const val = e.target.value;
                            setSpeedLimitVal(val);
                            localStorage.setItem("speed_limit_val", val);
                            const limitBps = calculateLimitBps(val, speedLimitUnit);
                            invoke("set_speed_limit", { limitBps }).catch(console.error);
                          }}
                        />
                        <select
                          className="spotlight-input"
                          style={{
                            padding: "6px 10px",
                            fontSize: "0.85rem",
                            fontWeight: 700,
                            color: "var(--accent-cyan)",
                            cursor: "pointer",
                            background: themeMode === "light" ? "#ffffff" : "rgba(0, 0, 0, 0.25)",
                            colorScheme: themeMode === "light" ? "light" : "dark",
                            borderRadius: "8px",
                            border: themeMode === "light" ? "1px solid rgba(0, 0, 0, 0.12)" : "1px solid rgba(255, 255, 255, 0.08)"
                          }}
                          value={speedLimitUnit}
                          onChange={(e) => {
                            const unit = e.target.value as "KB" | "MB" | "GB";
                            setSpeedLimitUnit(unit);
                            localStorage.setItem("speed_limit_unit", unit);
                            const limitBps = calculateLimitBps(speedLimitVal, unit);
                            invoke("set_speed_limit", { limitBps }).catch(console.error);
                          }}
                        >
                          <option value="KB" style={{ background: themeMode === "light" ? "#ffffff" : "#0d1117", color: themeMode === "light" ? "#000000" : "#ffffff" }}>KB/s</option>
                          <option value="MB" style={{ background: themeMode === "light" ? "#ffffff" : "#0d1117", color: themeMode === "light" ? "#000000" : "#ffffff" }}>MB/s</option>
                          <option value="GB" style={{ background: themeMode === "light" ? "#ffffff" : "#0d1117", color: themeMode === "light" ? "#000000" : "#ffffff" }}>GB/s</option>
                        </select>
                      </div>
                    </div>

                    <div style={{ display: "flex", gap: "8px", width: "100%", flexWrap: "wrap" }}>
                      {[
                        { label: "512 KB/s", val: "512", unit: "KB" },
                        { label: "1 MB/s", val: "1", unit: "MB" },
                        { label: "2 MB/s", val: "2", unit: "MB" },
                        { label: "5 MB/s", val: "5", unit: "MB" },
                        { label: "10 MB/s", val: "10", unit: "MB" },
                      ].map((preset) => {
                        const isSelected = speedLimitVal === preset.val && speedLimitUnit === preset.unit;
                        return (
                          <button
                            key={preset.label}
                            type="button"
                            className="hover-action-btn"
                            style={{
                              flex: 1,
                              padding: "6px 12px",
                              fontSize: "0.8rem",
                              fontWeight: 600,
                              borderRadius: "8px",
                              background: isSelected ? "rgba(6, 182, 212, 0.2)" : "rgba(255,255,255,0.04)",
                              color: isSelected ? "var(--accent-cyan)" : "var(--text-primary)",
                              borderColor: isSelected ? "var(--accent-cyan)" : "transparent"
                            }}
                            onClick={() => {
                              setSpeedLimitVal(preset.val);
                              setSpeedLimitUnit(preset.unit as "KB" | "MB" | "GB");
                              localStorage.setItem("speed_limit_val", preset.val);
                              localStorage.setItem("speed_limit_unit", preset.unit);
                              const limitBps = calculateLimitBps(preset.val, preset.unit);
                              invoke("set_speed_limit", { limitBps }).catch(console.error);
                            }}
                          >
                            {preset.label}
                          </button>
                        );
                      })}
                    </div>
                  </div>
                )}
              </div>

              <div className="settings-card">
                <div className="settings-section-header">
                  <Globe size={18} />
                  <span className="settings-section-title">Browser Integration</span>
                </div>
                <div className="settings-control-row">
                  <div className="settings-info-col">
                    <span className="settings-title">Intercept Browser Downloads</span>
                    <span className="settings-desc">Enable connection socket to capture video links and documents from browser extensions.</span>
                  </div>
                  <label className="switch-container">
                    <input
                      type="checkbox"
                      className="switch-input"
                      checked={interceptDownloads}
                      onChange={(e) => {
                        const val = e.target.checked;
                        setInterceptDownloads(val);
                        localStorage.setItem("intercept_downloads", String(val));
                        invoke("set_intercept_downloads", { enabled: val }).catch(console.error);
                      }}
                    />
                    <span className="switch-slider"></span>
                  </label>
                </div>
                <div className="form-group">
                  <span className="form-label">Native Messaging Bridge Port</span>
                  <input
                    type="number"
                    className="form-input"
                    value={integrationPort}
                    onChange={(e) => setIntegrationPort(parseInt(e.target.value))}
                  />
                </div>
              </div>

              <div className="settings-card">
                <div className="settings-section-header">
                  <Calendar size={18} />
                  <span className="settings-section-title">Download Scheduler</span>
                </div>
                <div className="settings-control-row">
                  <div className="settings-info-col">
                    <span className="settings-title">Enable Scheduler</span>
                    <span className="settings-desc">Schedule download files to trigger or pause within specific daily intervals.</span>
                  </div>
                  <label className="switch-container">
                    <input
                      type="checkbox"
                      className="switch-input"
                      checked={schedulerEnabled}
                      onChange={(e) => setSchedulerEnabled(e.target.checked)}
                    />
                    <span className="switch-slider"></span>
                  </label>
                </div>
                {schedulerEnabled && (
                  <div className="settings-grid">
                    <div className="form-group">
                      <span className="form-label">Start Time</span>
                      <input
                        type="time"
                        className="form-input"
                        value={startTime}
                        onChange={(e) => setStartTime(e.target.value)}
                      />
                    </div>
                    <div className="form-group">
                      <span className="form-label">End Time</span>
                      <input
                        type="time"
                        className="form-input"
                        value={endTime}
                        onChange={(e) => setEndTime(e.target.value)}
                      />
                    </div>
                    <div className="form-group settings-grid-full">
                      <span className="form-label">Repeat Days</span>
                      <div className="days-grid">
                        {daysOfWeek.map((day) => (
                          <div key={day} style={{ flex: 1 }}>
                            <input
                              type="checkbox"
                              id={`day-${day}`}
                              className="day-checkbox-input"
                              checked={activeDays.includes(day)}
                              onChange={() => toggleDay(day)}
                            />
                            <label htmlFor={`day-${day}`} className="day-checkbox-label">
                              {day}
                            </label>
                          </div>
                        ))}
                      </div>
                    </div>
                  </div>
                )}
              </div>

              <div className="visual-updater-card">
                <div className="visual-updater-header">
                  <div className="visual-updater-title">
                    <RefreshCw size={20} className={checkingUpdate || isDownloadingUpdate ? "animate-spin" : ""} style={{ color: "var(--accent-cyan)" }} />
                    <span>Software Updates</span>
                  </div>
                  <span style={{ padding: "4px 12px", borderRadius: "100px", background: "rgba(6, 182, 212, 0.15)", color: "var(--accent-cyan)", fontWeight: 700, fontSize: "0.85rem", border: "1px solid rgba(6, 182, 212, 0.3)" }}>
                    v{CURRENT_APP_VERSION}
                  </span>
                </div>

                <div className="settings-info-col">
                  <span className="settings-title">In-App Software Updater</span>
                  <span className="settings-desc">Check for signed application updates directly from GitHub Releases.</span>
                </div>

                {(isDownloadingUpdate || updateProgressInfo.status === "downloading" || updateProgressInfo.status === "installing") && (
                  <div style={{ display: "flex", flexDirection: "column", gap: "12px", width: "100%" }}>
                    {/* Liquid Flow Progress Bar */}
                    <div className="visual-updater-progress-track">
                      <div
                        className="visual-updater-progress-fill"
                        style={{ width: `${updateProgressInfo.percent}%` }}
                      />
                    </div>

                    {/* Metrics Grid */}
                    <div className="visual-updater-metrics">
                      <div className="updater-metric-box">
                        <span className="updater-metric-label">Downloaded</span>
                        <span className="updater-metric-value">{formatBytes(updateProgressInfo.downloaded)} / {formatBytes(updateProgressInfo.total)}</span>
                      </div>
                      <div className="updater-metric-box">
                        <span className="updater-metric-label">Speed</span>
                        <span className="updater-metric-value">{updateProgressInfo.speed > 0 ? `${formatBytes(updateProgressInfo.speed)}/s` : "---"}</span>
                      </div>
                      <div className="updater-metric-box">
                        <span className="updater-metric-label">Progress</span>
                        <span className="updater-metric-value">{updateProgressInfo.percent}%</span>
                      </div>
                      <div className="updater-metric-box">
                        <span className="updater-metric-label">ETA</span>
                        <span className="updater-metric-value">{updateProgressInfo.eta}</span>
                      </div>
                    </div>
                  </div>
                )}

                {/* OS-Sensitive Administrator / Superuser Privilege Banner */}
                {(updateProgressInfo.status === "installing" || updateProgressInfo.status === "waiting_auth") && (
                  <div className="superuser-warning-banner">
                    <ShieldAlert size={22} style={{ color: "var(--accent-orange)", flexShrink: 0, marginTop: "2px" }} />
                    <div className="superuser-warning-text">
                      <div className="superuser-warning-title">
                        {typeof navigator !== "undefined" && navigator.userAgent.toLowerCase().includes("win")
                          ? "Administrator Permission Required"
                          : "Administrator Privileges Required"
                        }
                      </div>
                      {typeof navigator !== "undefined" && navigator.userAgent.toLowerCase().includes("win")
                        ? <span>Windows User Account Control (UAC) is requesting administrator permission to complete the update. Please click <strong>'Yes'</strong> in the system prompt.</span>
                        : <span>Your operating system is requesting authorization to install the system package update. Please enter your <strong>Superuser (sudo) password</strong> in the system prompt to authorize installation.</span>
                      }
                    </div>
                  </div>
                )}

                {updateStatus && (
                  <div style={{
                    fontSize: "0.85rem",
                    color: updateStatus.includes("failed") ? "#ef4444" : "var(--accent-cyan)",
                    fontWeight: 600,
                    background: "rgba(0,0,0,0.2)",
                    padding: "10px 14px",
                    borderRadius: "10px",
                    width: "100%",
                    display: "flex",
                    alignItems: "center",
                    gap: "8px"
                  }}>
                    <Activity size={16} />
                    <span>{updateStatus}</span>
                  </div>
                )}

                <div style={{ display: "flex", justifyContent: "flex-end", width: "100%" }}>
                  <button
                    type="button"
                    className="accent-pill"
                    style={{ padding: "10px 24px", borderRadius: "100px", fontWeight: 700, fontSize: "0.9rem" }}
                    onClick={handleCheckForUpdates}
                    disabled={checkingUpdate || isDownloadingUpdate}
                  >
                    {checkingUpdate ? "Checking..." : (isDownloadingUpdate ? "Downloading Update..." : "Check for Updates")}
                  </button>
                </div>
              </div>
            </div>
          ) : (
            <div>
              {filteredDownloads.length === 0 ? (
                <div className="empty-state">
                  <Download size={48} strokeWidth={1} />
                  <div className="empty-title">No downloads found</div>
                  <p>There are no items matching this list. Add a link to get started.</p>
                </div>
              ) : (
                <div className="download-grid">
                  {sortedDownloads.map((d) => {
                    const statusText = getStatusText(d.status);
                    const isDownloading = statusText === "Downloading";
                    const isPaused = statusText === "Paused";
                    const isCompleted = statusText === "Completed";
                    const isFailed = statusText.startsWith("Failed");
                    const isTrash = statusText === "Trash";

                    const progressPercent = isCompleted
                      ? 100
                      : (d.total_size > 0
                        ? Math.min(100, Math.floor((d.downloaded / d.total_size) * 100))
                        : 0);
                    const catClass = `cat-${getFileCategory(d.filename)}`;

                    return (
                      <div
                        className="download-card-v2"
                        key={d.id.toString()}
                        onClick={() => setSelectedTask(d)}
                      >
                        <div className="card-left-v2">
                          <div className="liquid-progress-container">
                            <div
                              className={`liquid-fill ${isPaused ? "paused" : ""} ${isCompleted ? "completed" : ""} ${catClass}`}
                              style={{ transform: `translateY(${100 - progressPercent}%)` }}
                            />
                            <div className="circular-icon-inner">
                              {getFileIcon(d.filename)}
                            </div>
                          </div>
                        </div>

                        <div className="card-middle-v2">
                          <h3 className="file-name-v2" title={d.filename}>{d.filename}</h3>
                          <div className="file-meta-v2">
                            <span className={`status-pill-v2 status-${statusText.toLowerCase()}`}>{statusText}</span>
                            <span className="meta-divider">•</span>
                            <span>{formatBytes(d.downloaded)} / {d.total_size > 0 ? formatBytes(d.total_size) : "Unknown size"}</span>
                            {isDownloading && (
                              <>
                                <span className="meta-divider">•</span>
                                <span className="speed-text-v2">
                                  {formatBytes(d.speed)}/s
                                  {d.speed_limited && <span style={{ color: "var(--accent-orange)", fontSize: "0.65rem", background: "rgba(245, 158, 11, 0.15)", padding: "1px 5px", borderRadius: "4px", marginLeft: "6px", fontWeight: 700 }}>LIMITED</span>}
                                </span>
                                <span className="meta-divider">•</span>
                                <span>{d.eta}</span>
                              </>
                            )}
                          </div>
                        </div>

                        <div className="card-right-hover-actions">
                          {isTrash ? (
                            <>
                              {!d.file_exists && (
                                <button className="hover-action-btn" onClick={(e) => { e.stopPropagation(); handleRedownload(e, d.id); }} title="Re-download">
                                  <Play size={16} />
                                </button>
                              )}
                              <button className="hover-action-btn danger" onClick={(e) => { e.stopPropagation(); promptRemoveTask(e, d); }} title="Delete Permanently">
                                <Trash2 size={16} />
                              </button>
                            </>
                          ) : (
                            <>
                              {isDownloading && (
                                <button className="hover-action-btn" onClick={(e) => { e.stopPropagation(); handlePause(e, d.id); }} title="Pause">
                                  <Pause size={16} />
                                </button>
                              )}
                              {isPaused && (
                                <button className="hover-action-btn" onClick={(e) => { e.stopPropagation(); handleResume(e, d.id); }} title="Resume">
                                  <Play size={16} />
                                </button>
                              )}
                              {(isFailed || isPaused) && (
                                <button className="hover-action-btn" onClick={(e) => { e.stopPropagation(); handleRefreshLink(e, d.id); }} title="Refresh Link">
                                  <RefreshCw size={16} />
                                </button>
                              )}
                              {isFailed && !d.file_exists && (
                                <button className="hover-action-btn" onClick={(e) => { e.stopPropagation(); handleRedownload(e, d.id); }} title="Re-download">
                                  <Play size={16} />
                                </button>
                              )}
                              {isCompleted && (
                                <button className="hover-action-btn success" onClick={(e) => { e.stopPropagation(); handleOpenFileDir(e, d.save_path || ""); }} title="Open Folder">
                                  <FolderOpen size={16} />
                                </button>
                              )}
                              {isCompleted && !d.file_exists && (
                                <button className="hover-action-btn" onClick={(e) => { e.stopPropagation(); handleRedownload(e, d.id); }} title="Re-download">
                                  <Play size={16} />
                                </button>
                              )}
                              <button className="hover-action-btn danger" onClick={(e) => { e.stopPropagation(); promptRemoveTask(e, d); }} title="Delete">
                                <X size={16} />
                              </button>
                            </>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          )}
        </main>

        <footer className="branding-footer" style={{ padding: "12px", textAlign: "center", fontSize: "0.8rem" }}>
          A Product of <span style={{ color: "var(--accent-cyan)", fontWeight: 700, textShadow: "0 0 10px rgba(6, 182, 212, 0.2)" }}>Lumen Lab</span>, Designed & Developed by <span style={{ color: "var(--accent-orange)", fontWeight: 700, textShadow: "0 0 10px rgba(245, 158, 11, 0.2)" }}>Shaheer Ahmed</span>
        </footer>
      </div>

      {/* Main Window Modal: Add New Download (triggered manually) */}
      {showAddModal && (
        <div className="modal-backdrop-v2" onClick={() => setShowAddModal(false)}>
          <div className="modal-content-v2" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header-v2">
              <span className="modal-title-v2">New Download</span>
              <button className="modal-close-btn-v2" onClick={() => setShowAddModal(false)}>
                <X size={20} />
              </button>
            </div>

            <div className="modal-body-v2">
              <div className="form-group-v2">
                <span className="form-label-v2">Download URL</span>
                <input
                  type="text"
                  placeholder="Paste HTTP / HTTPS address..."
                  className="spotlight-input"
                  value={inputUrl}
                  onChange={(e) => handleUrlChange(e.target.value)}
                />
              </div>

              <div className="form-group-v2">
                <span className="form-label-v2">Save Filename</span>
                <input
                  type="text"
                  placeholder="Enter filename..."
                  className="spotlight-input"
                  value={customFilename}
                  onChange={(e) => setCustomFilename(e.target.value)}
                />
              </div>

              <div className="form-group-v2">
                <span className="form-label-v2">Save Location</span>
                <div style={{ display: "flex", gap: "12px" }}>
                  <input
                    type="text"
                    placeholder="Enter directory path..."
                    className="spotlight-input"
                    value={savePath}
                    onChange={(e) => setSavePath(e.target.value)}
                  />
                  <button className="hover-action-btn" style={{ width: "auto", padding: "0 20px" }} onClick={handlePickFolder}>Browse</button>
                </div>
              </div>
            </div>

            <div className="modal-actions-v2">
              <button className="hover-action-btn" style={{ width: "auto", padding: "0 24px" }} onClick={() => setShowAddModal(false)}>
                Cancel
              </button>
              <button
                className="accent-pill"
                style={{ padding: "12px 32px", borderRadius: "100px", fontWeight: 700, fontSize: "1rem" }}
                onClick={handleStartDownload}
                disabled={!inputUrl}
              >
                Start Download
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Drawer: Properties */}
      {selectedTask && (
        <div className="properties-drawer">
          <div className="drawer-header">
            <span className="drawer-title">File Details</span>
            <button className="modal-close-btn" onClick={() => setSelectedTask(null)}>
              <X size={18} />
            </button>
          </div>

          <div className="drawer-body">
            <div className="drawer-section">
              <span className="drawer-section-title">General Info</span>
              <div className="info-grid">
                <span className="info-label">Name:</span>
                <span className="info-value">{selectedTask.filename}</span>

                <span className="info-label">Size:</span>
                <span className="info-value">{formatBytes(selectedTask.total_size)}</span>

                <span className="info-label">Status:</span>
                <span className="info-value">{getStatusText(selectedTask.status)}</span>
              </div>
            </div>

            <div className="drawer-section">
              <span className="drawer-section-title">File Location</span>
              <div className="info-grid">
                <span className="info-label">URL Origin:</span>
                <span className="info-value">{selectedTask.url || "Unknown link origin"}</span>

                <span className="info-label">Target Directory:</span>
                <span className="info-value">{selectedTask.save_path || "Default downloads folder"}</span>
              </div>
            </div>

            <div className="drawer-section">
              <span className="drawer-section-title">Download Segments</span>
              <p style={{ fontSize: "0.8rem", color: "var(--text-secondary)" }}>
                Active chunk segments downloading concurrently in Rust:
              </p>
              <div className="segments-preview-grid">
                {[...Array(8)].map((_, idx) => {
                  const status = getStatusText(selectedTask.status);
                  const isCompleted = status === "Completed";
                  const isDownloading = status === "Downloading";

                  let blockClass = "";
                  if (isCompleted) blockClass = "completed";
                  else if (isDownloading) {
                    if (idx < 5) blockClass = "completed";
                    else if (idx === 5 || idx === 6) blockClass = "active";
                  }

                  return (
                    <div
                      key={idx}
                      className={`segment-block ${blockClass}`}
                      title={`Segment ${idx + 1}`}
                    />
                  );
                })}
              </div>
            </div>

            <div className="drawer-section">
              <span className="drawer-section-title">Checksum Hash</span>
              <div className="info-grid">
                <span className="info-label">MD5:</span>
                <span className="info-value" style={{ fontFamily: "monospace", fontSize: "0.8rem" }}>
                  {selectedTask.id.startsWith("mock") ? "2f9a74c2e64627a6c98ee403bf6506d2" : "Calculating on complete..."}
                </span>
              </div>
            </div>

            <div className="drawer-section" style={{ marginTop: "auto", borderTop: "1px solid rgba(255, 255, 255, 0.06)", paddingTop: "16px", display: "flex", gap: "10px", flexWrap: "wrap" }}>
              {getStatusText(selectedTask.status) === "Trash" ? (
                <>
                  {!selectedTask.file_exists && (
                    <button
                      className="action-btn"
                      style={{ flex: 1, color: "var(--accent-green)", borderColor: "rgba(16, 185, 129, 0.2)", background: "rgba(16, 185, 129, 0.05)" }}
                      onClick={(e) => handleRedownload(e, selectedTask.id)}
                    >
                      <Play size={14} />
                      <span>Re-download</span>
                    </button>
                  )}
                  <button className="action-btn action-btn-danger" style={{ flex: 1 }} onClick={(e) => promptRemoveTask(e, selectedTask)}>
                    <Trash2 size={14} />
                    <span>Delete Permanently</span>
                  </button>
                </>
              ) : (
                <>
                  {getStatusText(selectedTask.status) === "Downloading" && (
                    <button className="action-btn" style={{ flex: 1 }} onClick={(e) => handlePause(e, selectedTask.id)}>
                      <Pause size={14} />
                      <span>Pause</span>
                    </button>
                  )}
                  {getStatusText(selectedTask.status) === "Paused" && (
                    <button className="action-btn" style={{ flex: 1 }} onClick={(e) => handleResume(e, selectedTask.id)}>
                      <Play size={14} />
                      <span>Resume</span>
                    </button>
                  )}
                  {(getStatusText(selectedTask.status).startsWith("Failed") || getStatusText(selectedTask.status) === "Paused") && (
                    <button
                      className="action-btn"
                      style={{ flex: 1, color: "var(--accent-orange)", borderColor: "rgba(245, 158, 11, 0.2)", background: "rgba(245, 158, 11, 0.05)" }}
                      onClick={(e) => handleRefreshLink(e, selectedTask.id)}
                    >
                      <RefreshCw size={14} />
                      <span>Refresh Link</span>
                    </button>
                  )}
                  {getStatusText(selectedTask.status) === "Completed" && (
                    <button
                      className="action-btn"
                      style={{ flex: 1, color: "var(--accent-green)", borderColor: "rgba(16, 185, 129, 0.2)", background: "rgba(16, 185, 129, 0.05)" }}
                      onClick={(e) => handleOpenFileDir(e, selectedTask.save_path || "")}
                    >
                      <FolderOpen size={14} />
                      <span>Open Folder</span>
                    </button>
                  )}
                  {getStatusText(selectedTask.status) === "Completed" && !selectedTask.file_exists && (
                    <button className="action-btn" style={{ flex: 1 }} onClick={(e) => handleRedownload(e, selectedTask.id)}>
                      <Play size={14} />
                      <span>Re-download</span>
                    </button>
                  )}
                  <button className="action-btn action-btn-danger" style={{ flex: 1 }} onClick={(e) => promptRemoveTask(e, selectedTask)}>
                    <X size={14} />
                    <span>Delete</span>
                  </button>
                </>
              )}
            </div>
          </div>
        </div>
      )}

      {showRemoveConfirm && taskToRemove && (
        <div className="modal-backdrop-v2">
          <div className="modal-content-v2" style={{ borderTop: "2px solid var(--accent-red)" }}>
            <div className="modal-header-v2">
              <span className="modal-title-v2" style={{ color: "var(--accent-red)", display: "flex", alignItems: "center", gap: "12px", fontSize: "1.6rem" }}>
                <Trash2 size={24} strokeWidth={2.5} />
                {getStatusText(taskToRemove.status) === "Trash" ? "Permanent Deletion" : "Move to Trash"}
              </span>
              <button className="modal-close-btn-v2" onClick={() => { setShowRemoveConfirm(false); setTaskToRemove(null); }}>
                <X size={20} />
              </button>
            </div>

            <div className="modal-body-v2">
              <p style={{ fontSize: "1rem", color: "var(--text-primary)", lineHeight: "1.6", margin: 0, fontWeight: 500 }}>
                {getStatusText(taskToRemove.status) === "Trash"
                  ? `You are about to permanently delete "${taskToRemove.filename}". This action cannot be undone.`
                  : `Are you sure you want to move "${taskToRemove.filename}" to the Trash?`
                }
              </p>

              {getStatusText(taskToRemove.status) === "Completed" && (
                <label className="custom-checkbox-container" style={{ marginTop: "12px", padding: "16px", background: "rgba(255,255,255,0.03)", borderRadius: "12px", border: "1px solid rgba(255,255,255,0.05)" }}>
                  <input
                    type="checkbox"
                    checked={deleteFileFromDisk}
                    onChange={(e) => setDeleteFileFromDisk(e.target.checked)}
                  />
                  <span className="custom-checkbox-checkmark"></span>
                  <span className="custom-checkbox-label" style={{ fontSize: "0.95rem" }}>Also delete the downloaded file from disk</span>
                </label>
              )}
            </div>

            <div className="modal-actions-v2">
              <button className="hover-action-btn" style={{ width: "auto", padding: "0 24px" }} onClick={() => { setShowRemoveConfirm(false); setTaskToRemove(null); }}>
                Cancel
              </button>
              <button
                className="accent-pill danger-pill"
                onClick={confirmRemoveTask}
              >
                Confirm Delete
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Update Success Celebration Upgrade Modal */}
      {showUpdateSuccessModal && (
        <div className="modal-backdrop-v2" onClick={() => setShowUpdateSuccessModal(false)}>
          <div className="celebration-modal-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="celebration-badge-header">
              <div className="celebration-icon-wrapper">
                <Sparkles size={28} />
              </div>
              <div>
                <div className="celebration-title-text" style={{ fontSize: "1.2rem", lineHeight: "1.35" }}>
                  {updatedFromVersion
                    ? `🎉 v${updatedFromVersion} is history. v${CURRENT_APP_VERSION} is live now with fresh upgrades and smoother vibes!`
                    : `🎉 v${CURRENT_APP_VERSION} is live now with fresh upgrades and smoother vibes!`
                  }
                </div>
                <div style={{ fontSize: "0.88rem", color: "var(--text-secondary)", marginTop: "2px" }}>
                  Impressive Download Manager has been upgraded with major engine enhancements!
                </div>
              </div>
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
              <div className="celebration-feature-card">
                <Zap size={20} style={{ color: "var(--accent-cyan)", flexShrink: 0, marginTop: "2px" }} />
                <div>
                  <div style={{ fontWeight: 800, fontSize: "0.92rem", color: "var(--text-primary)" }}>Full Wire-Speed Download Engine</div>
                  <div style={{ fontSize: "0.82rem", color: "var(--text-secondary)", marginTop: "2px" }}>Multi-threaded connection engine with zero lock contention and TCP socket nodelay tuning for maximum throughput.</div>
                </div>
              </div>

              <div className="celebration-feature-card">
                <Clock size={20} style={{ color: "var(--accent-cyan)", flexShrink: 0, marginTop: "2px" }} />
                <div>
                  <div style={{ fontWeight: 800, fontSize: "0.92rem", color: "var(--text-primary)" }}>Instant Popup Capture (&lt; 50ms)</div>
                  <div style={{ fontSize: "0.82rem", color: "var(--text-secondary)", marginTop: "2px" }}>Download popups open instantly while network HEAD probes and 302 redirect tracking run asynchronously in background.</div>
                </div>
              </div>

              <div className="celebration-feature-card">
                <Gauge size={20} style={{ color: "var(--accent-cyan)", flexShrink: 0, marginTop: "2px" }} />
                <div>
                  <div style={{ fontWeight: 800, fontSize: "0.92rem", color: "var(--text-primary)" }}>Precision Bandwidth Limiter</div>
                  <div style={{ fontSize: "0.82rem", color: "var(--text-secondary)", marginTop: "2px" }}>Free text speed entry + KB/s, MB/s, GB/s unit selector dropdown & token bucket sliding window algorithm.</div>
                </div>
              </div>

              <div className="celebration-feature-card">
                <ShieldCheck size={20} style={{ color: "var(--accent-cyan)", flexShrink: 0, marginTop: "2px" }} />
                <div>
                  <div style={{ fontWeight: 800, fontSize: "0.92rem", color: "var(--text-primary)" }}>Protected Auto-Restart Queue</div>
                  <div style={{ fontSize: "0.82rem", color: "var(--text-secondary)", marginTop: "2px" }}>Updates download seamlessly in background and automatically queue restart until active downloads complete.</div>
                </div>
              </div>
            </div>

            <div style={{ display: "flex", justifyContent: "flex-end", marginTop: "6px" }}>
              <button
                className="accent-pill"
                style={{ padding: "12px 32px", borderRadius: "100px", fontWeight: 800, fontSize: "0.95rem", background: "linear-gradient(135deg, #06b6d4, #3b82f6)", boxShadow: "0 4px 20px rgba(6, 182, 212, 0.4)" }}
                onClick={() => setShowUpdateSuccessModal(false)}
              >
                Explore v{CURRENT_APP_VERSION} 🚀
              </button>
            </div>
          </div>
        </div>
      )}

      {activeTooltip && (
        <div
          className="global-tooltip-v2"
          style={{
            left: `${activeTooltip.x}px`,
            top: `${activeTooltip.y}px`
          }}
        >
          {activeTooltip.title}
        </div>
      )}
    </div>
  );
}

export default App;
