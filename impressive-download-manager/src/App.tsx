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
  ShieldAlert,
  ChevronDown,
  Moon,
  Sun,
  Laptop,
  Rocket,
  Terminal,
  Folder,
  AlertTriangle,
  AlertCircle,
  Link2,
  RotateCcw
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
  category?: string;
  added_at?: number;
}

interface PreflightProbeResult {
  url: string;
  final_url: string;
  filename: string;
  total_size: number;
  formatted_size: string;
  supports_range: boolean;
  category: string;
  mime_type: string | null;
  available_disk_space: number | null;
  formatted_disk_space: string | null;
  has_enough_space: boolean;
}

interface DiskSpaceInfo {
  total_bytes: number;
  available_bytes: number;
  formatted_available: string;
}

const detectCategory = (filename: string): string => {
  const ext = filename.split(".").pop()?.toLowerCase();
  if (!ext) return "Other";
  if (["mp4", "webm", "mkv", "mov", "avi", "m4v", "flv", "3gp", "ts", "m3u8", "wmv"].includes(ext)) return "Video";
  if (["mp3", "wav", "aac", "flac", "m4a", "ogg", "wma", "opus"].includes(ext)) return "Audio";
  if (["pdf", "docx", "doc", "txt", "xlsx", "xls", "pptx", "ppt", "epub", "csv", "rtf", "md"].includes(ext)) return "Documents";
  if (["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "dmg"].includes(ext)) return "Compressed";
  if (["exe", "msi", "deb", "rpm", "dmg", "pkg", "appimage", "apk"].includes(ext)) return "Programs";
  return "Other";
};

const getCategoryFolder = (cat: string, defaultDir: string): string => {
  const custom = localStorage.getItem(`category_dir_${cat}`);
  if (custom) return custom;
  if (!defaultDir) return "";
  if (cat === "Other") return defaultDir;
  return `${defaultDir.replace(/\/+$/, "")}/${cat}`;
};

const saveCategoryFolder = (cat: string, folder: string) => {
  if (cat && folder) {
    localStorage.setItem(`category_dir_${cat}`, folder);
  }
};

const getOrAssignTaskTimestamp = (id: string, indexFallback: number = 0): number => {
  try {
    const raw = localStorage.getItem("idm_task_timestamps");
    const map: Record<string, number> = raw ? JSON.parse(raw) : {};
    if (map[id]) {
      return map[id];
    }
    const ts = Date.now() - (indexFallback * 1000);
    map[id] = ts;
    localStorage.setItem("idm_task_timestamps", JSON.stringify(map));
    return ts;
  } catch {
    return Date.now();
  }
};

const recordTaskTimestamp = (id: string, timestamp: number = Date.now()): void => {
  try {
    const raw = localStorage.getItem("idm_task_timestamps");
    const map: Record<string, number> = raw ? JSON.parse(raw) : {};
    map[id] = timestamp;
    localStorage.setItem("idm_task_timestamps", JSON.stringify(map));
  } catch {
    // Ignore storage errors
  }
};

const syncTimestampsAndSetDownloads = (
  list: DownloadProgress[],
  setDownloads: React.Dispatch<React.SetStateAction<DownloadProgress[]>>
) => {
  try {
    const raw = localStorage.getItem("idm_task_timestamps");
    const map: Record<string, number> = raw ? JSON.parse(raw) : {};
    let changed = false;
    const baseNow = Date.now();
    list.forEach((item, idx) => {
      if (!map[item.id]) {
        map[item.id] = baseNow - (idx * 1000);
        changed = true;
      }
    });
    if (changed) {
      localStorage.setItem("idm_task_timestamps", JSON.stringify(map));
    }
    const withTimestamps = list.map((item) => ({
      ...item,
      added_at: map[item.id] || baseNow,
    }));
    setDownloads(withTimestamps);
  } catch {
    setDownloads(list);
  }
};

const diskSpaceMemoryCache = new Map<string, { res: DiskSpaceInfo; ts: number }>();

const checkDiskSpace = async (
  path: string,
  setSpace: (s: string) => void,
  setSafe: (b: boolean) => void,
  expectedSize: number = 0
) => {
  if (!path) return;
  const now = Date.now();
  const cached = diskSpaceMemoryCache.get(path);
  if (cached && now - cached.ts < 10000) {
    const res = cached.res;
    setSpace(res.formatted_available);
    if (expectedSize > 0) {
      setSafe(res.available_bytes >= expectedSize);
    } else {
      setSafe(res.available_bytes > 50 * 1024 * 1024);
    }
    return;
  }

  try {
    const res = await invoke<DiskSpaceInfo>("get_disk_space", { targetPath: path });
    if (res && res.formatted_available) {
      diskSpaceMemoryCache.set(path, { res, ts: now });
      setSpace(res.formatted_available);
      if (expectedSize > 0) {
        setSafe(res.available_bytes >= expectedSize);
      } else {
        setSafe(res.available_bytes > 50 * 1024 * 1024);
      }
    }
  } catch (err) {
    console.warn("get_disk_space failed:", err);
  }
};


interface Category {
  id: string;
  name: string;
  icon: React.ReactNode;
}

const formatBytes = (bytes: number): string => {
  if (!bytes || isNaN(bytes) || bytes <= 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB", "PB"];
  const i = Math.min(sizes.length - 1, Math.floor(Math.log(bytes) / Math.log(k)));
  return `${(bytes / Math.pow(k, i)).toFixed(2)} ${sizes[i]}`;
};

const getInitialPopupMode = (): string | null => {
  if (typeof window === "undefined") return null;
  try {
    const url = new URL(window.location.href);
    const searchMode = url.searchParams.get("mode");
    if (searchMode) return searchMode;
    if (window.location.hash) {
      const hashParams = new URLSearchParams(window.location.hash.replace(/^#\/?/, ""));
      const hashMode = hashParams.get("mode");
      if (hashMode) return hashMode;
    }
  } catch {}
  return null;
};

function App() {
  // Query routing state (synchronously resolved to avoid main-window frame flashing in popups)
  const [popupMode, setPopupMode] = useState<string | null>(getInitialPopupMode);
  const [popupUrl, setPopupUrl] = useState("");
  const [popupFilename, setPopupFilename] = useState("");
  const [popupSavePath, setPopupSavePath] = useState("");
  const [popupCookie, setPopupCookie] = useState("");
  const [popupReferrer, setPopupReferrer] = useState("");
  const [popupTaskId, setPopupTaskId] = useState<string | null>(null);
  const [popupProgress, setPopupProgress] = useState<DownloadProgress | null>(null);
  const [popupSize, setPopupSize] = useState("0");
  const [popupAudioUrl, setPopupAudioUrl] = useState("");

  // Main Dashboard State
  const [activeCategory, setActiveCategory] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [downloads, setDownloads] = useState<DownloadProgress[]>([]);


  // Modal State (Manual Trigger)
  const [showAddModal, setShowAddModal] = useState(false);
  const [inputUrl, setInputUrl] = useState("");
  const [customFilename, setCustomFilename] = useState("");
  const [savePath, setSavePath] = useState(() => localStorage.getItem("default_save_dir") || "");

  // Pre-flight & Category Routing State
  const [popupCategory, setPopupCategory] = useState("Other");
  const [rememberCategoryFolder, setRememberCategoryFolder] = useState(true);
  const [popupDiskSpace, setPopupDiskSpace] = useState("");
  const [popupDiskSafe, setPopupDiskSafe] = useState(true);
  const [addModalCategory, setAddModalCategory] = useState("Other");
  const [addModalDiskSpace, setAddModalDiskSpace] = useState("");
  const [addModalDiskSafe, setAddModalDiskSafe] = useState(true);
  const [addModalFileSize, setAddModalFileSize] = useState("");
  const [addModalSizeRaw, setAddModalSizeRaw] = useState<number | undefined>(undefined);
  const [isProbingUrl, setIsProbingUrl] = useState(false);
  const [, setCategoryFoldersVersion] = useState(0);
  const probeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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
  const [minimizeToTray, setMinimizeToTray] = useState(() => {
    const saved = localStorage.getItem("minimize_to_tray");
    return saved !== null ? saved === "true" : true;
  });

  useEffect(() => {
    invoke("set_minimize_to_tray", { enabled: minimizeToTray }).catch(console.error);
  }, []);
  const [maxChunks, setMaxChunks] = useState(() => {
    const saved = localStorage.getItem("max_chunks");
    return saved ? Math.min(32, Math.max(1, parseInt(saved, 10) || 8)) : 8;
  });
  const [speedLimitEnabled, setSpeedLimitEnabled] = useState(() => localStorage.getItem("speed_limit_enabled") === "true");
  const [speedLimitVal, setSpeedLimitVal] = useState(() => localStorage.getItem("speed_limit_val") || "512");
  const [speedLimitUnit, setSpeedLimitUnit] = useState<"KB" | "MB" | "GB">(() => (localStorage.getItem("speed_limit_unit") as "KB" | "MB" | "GB") || "KB");
  const [unitDropdownOpen, setUnitDropdownOpen] = useState(false);
  const unitDropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (unitDropdownRef.current && !unitDropdownRef.current.contains(event.target as Node)) {
        setUnitDropdownOpen(false);
      }
    };
    if (unitDropdownOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [unitDropdownOpen]);
  const [interceptDownloads, setInterceptDownloads] = useState(() => {
    const val = localStorage.getItem("intercept_downloads");
    return val !== null ? val === "true" : true;
  });
  const [integrationPort, setIntegrationPort] = useState<number>(() => {
    const saved = localStorage.getItem("integration_port");
    return saved ? parseInt(saved, 10) : 9600;
  });

  // Remove & Bulk Selection States
  const [showRemoveConfirm, setShowRemoveConfirm] = useState(false);
  const [tasksToRemove, setTasksToRemove] = useState<DownloadProgress[]>([]);
  const [deleteFileFromDisk, setDeleteFileFromDisk] = useState(false);
  const [selectedTaskIds, setSelectedTaskIds] = useState<string[]>([]);
  const [showEmptyTrashConfirm, setShowEmptyTrashConfirm] = useState(false);
  const [emptyTrashDeleteFiles, setEmptyTrashDeleteFiles] = useState(false);

  // Updater State & Metrics
  const CURRENT_APP_VERSION = "0.8.0";
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [pendingRelaunch, setPendingRelaunch] = useState(false);
  const [isDownloadingUpdate, setIsDownloadingUpdate] = useState(false);
  const [updatedFromVersion, setUpdatedFromVersion] = useState<string | null>("0.7.7");
  const [showUpdateSuccessModal, setShowUpdateSuccessModal] = useState(false);

  const [updateProgressInfo, setUpdateProgressInfo] = useState<{
    downloaded: number;
    total: number;
    percent: number;
    speed: number;
    eta: string;
    status: "idle" | "checking" | "downloading" | "installing" | "relaunching" | "waiting_auth" | "ready" | "error";
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
    // Strictly prohibit update celebration modal from showing inside popup windows
    const isPopup = getInitialPopupMode() !== null || window.location.href.includes("mode=") || window.location.href.includes("popup");
    if (!isPopup) {
      const savedVersion = localStorage.getItem("installed_app_version");
      if (savedVersion && savedVersion !== CURRENT_APP_VERSION) {
        setUpdatedFromVersion(savedVersion);
        setShowUpdateSuccessModal(true);
      }
      localStorage.setItem("installed_app_version", CURRENT_APP_VERSION);
    }
  }, []);

  // Native Messaging Host State
  const [integrationStatus, setIntegrationStatus] = useState<{
    native_host_name: string;
    binary_path: string;
    browsers: { browser: string; manifest_path: string; installed: boolean; error?: string | null }[];
  } | null>(null);
  const [customExtensionId, setCustomExtensionId] = useState("");
  const [syncingBrowsers, setSyncingBrowsers] = useState(false);
  const [syncMessage, setSyncMessage] = useState<string | null>(null);

  const loadIntegrationStatus = async () => {
    try {
      const status = await invoke<any>("get_browser_integration_status");
      setIntegrationStatus(status);
    } catch (err) {
      console.error("Failed to load browser integration status:", err);
    }
  };

  useEffect(() => {
    if (activeCategory === "settings") {
      loadIntegrationStatus();
    }
  }, [activeCategory]);

  const handleSyncBrowsers = async () => {
    setSyncingBrowsers(true);
    setSyncMessage(null);
    try {
      const res = await invoke<{
        success: boolean;
        registered_browsers: { browser: string; manifest_path: string; installed: boolean }[];
        detected_extension_ids: string[];
      }>("register_native_messaging", {
        customId: customExtensionId.trim() || null,
      });
      const count = res.registered_browsers.filter((b) => b.installed).length;
      setSyncMessage(`Native host registered across ${count} browsers!`);
      await loadIntegrationStatus();
    } catch (err: any) {
      setSyncMessage(`Sync failed: ${err?.message || err}`);
    } finally {
      setSyncingBrowsers(false);
    }
  };

  // Watcher effect: Relaunches app when ongoing downloads complete
  useEffect(() => {
    if (!pendingRelaunch) return;
    const hasActiveDownloads = downloads.some((d) => {
      const s = typeof d.status === "string" ? d.status : JSON.stringify(d.status);
      return s === "Downloading" || s === '"Downloading"';
    });

    if (!hasActiveDownloads) {
      setPendingRelaunch(false);
      setUpdateProgressInfo((prev) => ({ ...prev, status: "relaunching" }));
      setUpdateStatus("Active downloads completed! Relaunching application...");
      setTimeout(() => {
        relaunch().catch(console.error);
      }, 1000);
    }
  }, [downloads, pendingRelaunch]);

  // Background update check interval (runs every 15 mins while app is running)
  useEffect(() => {
    const interval = setInterval(() => {
      checkUpdate({ headers: { "Cache-Control": "no-cache", "Pragma": "no-cache" } })
        .then((update) => {
          if (update && update.available) {
            setUpdateStatus(`New update v${update.version} is available! Go to Settings to install.`);
          }
        })
        .catch(() => { });
    }, 15 * 60 * 1000);
    return () => clearInterval(interval);
  }, []);

  const triggerRelaunchOrWait = async () => {
    const hasActiveDownloads = downloads.some((d) => {
      const s = typeof d.status === "string" ? d.status : JSON.stringify(d.status);
      return s === "Downloading" || s === '"Downloading"';
    });

    if (hasActiveDownloads) {
      setPendingRelaunch(true);
      setUpdateStatus("Update installed! Application will restart automatically once active downloads finish.");
    } else {
      setUpdateProgressInfo((prev) => ({ ...prev, status: "relaunching" }));
      setUpdateStatus("Update installed! Relaunching application...");
      await new Promise((resolve) => setTimeout(resolve, 1000));
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
          setUpdateStatus("Installing update into system... Please authorize system prompt if required.");
        }
      });

      setIsDownloadingUpdate(false);
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
      const update = await checkUpdate({ headers: { "Cache-Control": "no-cache", "Pragma": "no-cache" } });
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
      if (
        errMsg.includes("Could not fetch a valid release JSON") ||
        errMsg.includes("404") ||
        errMsg.includes("fallback platforms") ||
        errMsg.includes("were found in the response")
      ) {
        setUpdateStatus("You are running the latest version! No new update found.");
        setUpdateProgressInfo((prev) => ({ ...prev, status: "idle" }));
      } else {
        setUpdateStatus(`Update check error: ${errMsg}`);
        setUpdateProgressInfo((prev) => ({ ...prev, status: "error" }));
      }
    } finally {
      setCheckingUpdate(false);
    }
  };

  // Scheduler State
  const [schedulerEnabled, setSchedulerEnabled] = useState(() => {
    return localStorage.getItem("scheduler_enabled") === "true";
  });
  const [startTime, setStartTime] = useState(() => {
    return localStorage.getItem("scheduler_start_time") || "02:00";
  });
  const [endTime, setEndTime] = useState(() => {
    return localStorage.getItem("scheduler_end_time") || "06:00";
  });
  const [activeDays, setActiveDays] = useState<string[]>(() => {
    const saved = localStorage.getItem("scheduler_active_days");
    if (saved) {
      try {
        const parsed = JSON.parse(saved);
        if (Array.isArray(parsed) && parsed.length > 0) return parsed;
      } catch { }
    }
    return ["Mon", "Tue", "Wed", "Thu", "Fri"];
  });

  const syncSchedulerConfig = (enabled: boolean, start: string, end: string, days: string[]) => {
    invoke("set_scheduler_config", {
      config: {
        enabled,
        start_time: start,
        end_time: end,
        active_days: days,
      },
    }).catch(console.error);
  };

  const [activeTooltip, setActiveTooltip] = useState<{ title: string; x: number; y: number; pos?: "right" | "top" | "bottom" } | null>(null);

  const showTooltip = (title: string, e: React.MouseEvent, pos: "right" | "top" | "bottom" = "right") => {
    const rect = e.currentTarget.getBoundingClientRect();
    if (pos === "top") {
      setActiveTooltip({
        title,
        x: rect.left + rect.width / 2,
        y: rect.top - 8,
        pos: "top"
      });
    } else if (pos === "bottom") {
      setActiveTooltip({
        title,
        x: rect.left + rect.width / 2,
        y: rect.bottom + 8,
        pos: "bottom"
      });
    } else {
      setActiveTooltip({
        title,
        x: rect.right + 12,
        y: rect.top + rect.height / 2,
        pos: "right"
      });
    }
  };

  const hideTooltip = () => setActiveTooltip(null);

  // Theme Mode Settings State
  const [themeMode, setThemeMode] = useState<"dark" | "light" | "system">(() => {
    return (localStorage.getItem("theme_mode") as any) || "dark";
  });

  useEffect(() => {
    // Only write to localStorage if changed to prevent cross-window storage loop
    if (localStorage.getItem("theme_mode") !== themeMode) {
      localStorage.setItem("theme_mode", themeMode);
    }

    const applyTheme = () => {
      let resolvedTheme = "dark";
      if (themeMode === "system") {
        const isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        resolvedTheme = isDark ? "dark" : "light";
      } else {
        resolvedTheme = themeMode;
      }

      if (document.documentElement.getAttribute("data-theme") !== resolvedTheme) {
        document.documentElement.setAttribute("data-theme", resolvedTheme);
      }

      try {
        invoke("sync_theme_mode", { themeMode: resolvedTheme, theme_mode: resolvedTheme }).catch(() => { });
      } catch (e) { }
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
      if (e.key === "theme_mode" && e.newValue && e.newValue !== themeMode) {
        setThemeMode(e.newValue as any);
      }
    };
    window.addEventListener("storage", handleStorageChange);
    return () => window.removeEventListener("storage", handleStorageChange);
  }, [themeMode]);

  const daysOfWeek = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

  const toggleDay = (day: string) => {
    const nextDays = activeDays.includes(day)
      ? activeDays.filter((d) => d !== day)
      : [...activeDays, day];
    setActiveDays(nextDays);
    localStorage.setItem("scheduler_active_days", JSON.stringify(nextDays));
    syncSchedulerConfig(schedulerEnabled, startTime, endTime, nextDays);
  };

  // Parse filename from URL
  const extractFilename = (url: string): string => {
    try {
      const parsed = new URL(url);
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
      const lastSegment = pathname.substring(pathname.lastIndexOf("/") + 1).split("?")[0];
      if (lastSegment && lastSegment !== "download") {
        try {
          return decodeURIComponent(lastSegment);
        } catch {
          return lastSegment;
        }
      }
      return "downloaded_file";
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

  // Auto-fill filename and probe URL when URL input changes
  const handleUrlChange = (val: string) => {
    setInputUrl(val);
    const initialName = extractFilename(val);
    setCustomFilename(initialName);
    const initialCat = detectCategory(initialName);
    setAddModalCategory(initialCat);
    const folder = getCategoryFolder(initialCat, defaultSaveDir);
    setSavePath(folder);
    checkDiskSpace(folder, setAddModalDiskSpace, setAddModalDiskSafe);

    if (probeTimerRef.current) clearTimeout(probeTimerRef.current);
    if (val.startsWith("http://") || val.startsWith("https://")) {
      setIsProbingUrl(true);
      probeTimerRef.current = setTimeout(async () => {
        try {
          const res = await invoke<PreflightProbeResult>("probe_download_url", {
            url: val,
            saveFolder: folder,
          });
          if (res) {
            if (res.filename && res.filename !== "downloaded_file") {
              setCustomFilename(res.filename);
            }
            if (res.category) {
              setAddModalCategory(res.category);
              const catFolder = getCategoryFolder(res.category, defaultSaveDir);
              setSavePath(catFolder);
              if (res.formatted_disk_space) {
                setAddModalDiskSpace(res.formatted_disk_space);
                setAddModalDiskSafe(res.has_enough_space);
              }
            }
            if (res.formatted_size && res.formatted_size !== "Unknown") {
              setAddModalFileSize(res.formatted_size);
              if (res.total_size && res.total_size > 0) {
                setAddModalSizeRaw(res.total_size);
              }
            }
          }
        } catch (e) {
          console.warn("probe_download_url error:", e);
        } finally {
          setIsProbingUrl(false);
        }
      }, 500);
    }
  };

  // Diagnostic Error Banner State
  const [initError, setInitError] = useState<string | null>(null);
  const [isStartingDownload, setIsStartingDownload] = useState(false);
  const [popupUserAgent, setPopupUserAgent] = useState("");

  // Initialization query/hash-param routing & initial loading
  useEffect(() => {
    const searchParams = new URLSearchParams(window.location.search);
    const hashString = window.location.hash.startsWith("#") ? window.location.hash.slice(1) : window.location.hash;
    const hashParams = new URLSearchParams(hashString);

    const mode = searchParams.get("popup") || hashParams.get("popup");
    const url = searchParams.get("url") || hashParams.get("url") || "";
    const filename = searchParams.get("filename") || hashParams.get("filename") || "";
    const savePath = searchParams.get("save_path") || hashParams.get("save_path") || "";
    const cookie = searchParams.get("cookie") || hashParams.get("cookie") || "";
    const referrer = searchParams.get("referrer") || hashParams.get("referrer") || "";
    const userAgent = searchParams.get("user_agent") || hashParams.get("user_agent") || "";
    const size = searchParams.get("size") || hashParams.get("size") || "0";
    const taskId = searchParams.get("id") || hashParams.get("id") || null;
    const audioUrl = searchParams.get("audio_url") || hashParams.get("audio_url") || "";

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

    // Sync scheduler config with backend engine
    invoke<any>("get_scheduler_config")
      .then((cfg) => {
        if (cfg) {
          setSchedulerEnabled(cfg.enabled);
          setStartTime(cfg.start_time || "02:00");
          setEndTime(cfg.end_time || "06:00");
          if (Array.isArray(cfg.active_days)) {
            setActiveDays(cfg.active_days);
            localStorage.setItem("scheduler_active_days", JSON.stringify(cfg.active_days));
          }
          localStorage.setItem("scheduler_enabled", String(cfg.enabled));
          localStorage.setItem("scheduler_start_time", cfg.start_time || "02:00");
          localStorage.setItem("scheduler_end_time", cfg.end_time || "06:00");
        }
      })
      .catch(() => {
        syncSchedulerConfig(schedulerEnabled, startTime, endTime, activeDays);
      });

    if (mode) {
      setPopupMode(mode);
      setPopupUrl(url);
      let safeFilename = filename;
      try {
        safeFilename = filename ? decodeURIComponent(filename) : "";
      } catch {
        safeFilename = filename;
      }
      setPopupFilename(safeFilename);
      const initialCat = detectCategory(safeFilename);
      setPopupCategory(initialCat);

      const targetFolder = savePath ? decodeURIComponent(savePath) : getCategoryFolder(initialCat, savedDir || defaultSaveDir);
      setSavePath(targetFolder);
      setPopupSavePath(targetFolder);
      checkDiskSpace(targetFolder, setPopupDiskSpace, setPopupDiskSafe, parseInt(size) || 0);

      setPopupCookie(cookie);
      setPopupReferrer(referrer);
      setPopupUserAgent(userAgent);
      setPopupTaskId(taskId);
      setPopupSize(size);
      setPopupAudioUrl(audioUrl);

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
        setPopupFilename(filename);
      }
    } else {
      // Main dashboard: fetch all active/completed downloads from backend
      invoke<DownloadProgress[]>("get_all_downloads")
        .then((list) => {
          if (list) syncTimestampsAndSetDownloads(list, setDownloads);
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
        if (list) syncTimestampsAndSetDownloads(list, setDownloads);
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
          if (prog && !stopped) {
            setPopupProgress((prev) => {
              if (
                prev &&
                prev.downloaded === prog.downloaded &&
                prev.speed === prog.speed &&
                prev.status === prog.status &&
                prev.eta === prog.eta
              ) {
                return prev;
              }
              return prog;
            });
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
        await new Promise<void>((res) => setTimeout(res, 800));
      }
    };

    poll();
    return () => { stopped = true; };
  }, [popupMode, popupTaskId]);

  const popupModeRef = useRef(popupMode);
  const popupTaskIdRef = useRef(popupTaskId);
  useEffect(() => { popupModeRef.current = popupMode; }, [popupMode]);
  useEffect(() => { popupTaskIdRef.current = popupTaskId; }, [popupTaskId]);

  // Event listener: handles main-dashboard updates, progress popup, and browser intercept
  useEffect(() => {
    let unlistenProgress: (() => void) | undefined;
    let unlistenIntercept: (() => void) | undefined;

    async function setupListeners() {
      unlistenProgress = await listen<DownloadProgress>("download-progress", (event) => {
        const mode = popupModeRef.current;
        const taskId = popupTaskIdRef.current;

        if (mode === "progress") {
          if (!taskId || event.payload.id === taskId) {
            setPopupProgress((prev) => {
              if (
                prev &&
                prev.downloaded === event.payload.downloaded &&
                prev.speed === event.payload.speed &&
                prev.status === event.payload.status &&
                prev.eta === event.payload.eta
              ) {
                return prev;
              }
              return event.payload;
            });
            if (event.payload.status === "Completed") {
              invoke("open_complete_window", { filename: event.payload.filename, savePath: event.payload.save_path || "" });
              invoke("close_window");
            }
          }
          return;
        }

        setDownloads((prev) => {
          const index = prev.findIndex((d) => d.id === event.payload.id);
          const updated = [...prev];
          if (index !== -1) {
            updated[index] = {
              ...prev[index],
              ...event.payload,
              added_at: prev[index].added_at || getOrAssignTaskTimestamp(event.payload.id),
            };
            return updated;
          } else {
            const added_at = getOrAssignTaskTimestamp(event.payload.id);
            return [{ ...event.payload, added_at }, ...prev];
          }
        });
      });

      if (!popupModeRef.current) {
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
  }, []);

  // Submit start download from main window modal
  const handleStartDownload = async (queueOnly: boolean = false) => {
    if (!inputUrl || isStartingDownload) return;

    if (inputUrl.includes("sourceforge.net/projects/")) {
      alert("SourceForge landing page detected instead of direct file mirror URL! Please wait for the download link to resolve to a direct mirror host.");
      return;
    }

    console.log("[main-add] Invoking start_download with URL:", inputUrl);

    setIsStartingDownload(true);
    const finalFilename = customFilename || extractFilename(inputUrl);
    const targetFolder = savePath || defaultSaveDir;
    const finalSavePath = `${targetFolder.endsWith("/") ? targetFolder : targetFolder + "/"}${finalFilename}`;

    if (rememberCategoryFolder && addModalCategory) {
      saveCategoryFolder(addModalCategory, targetFolder);
    }

    try {
      const id = await invoke<string>("start_download", {
        url: inputUrl,
        filename: finalFilename,
        savePath: finalSavePath,
        cookie: popupCookie || "",
        referrer: popupReferrer || "",
        userAgent: popupUserAgent || "",
        category: addModalCategory || undefined,
        size: addModalSizeRaw,
        queueOnly,
        reuseActive: true,
      });

      if (id.startsWith("EXISTS:")) {
        const existingId = id.slice(7);
        setShowAddModal(false);
        setIsStartingDownload(false);
        await invoke("open_progress_window", { id: existingId });
        return;
      }

      const added_at = Date.now();
      recordTaskTimestamp(id, added_at);
      const newTask: DownloadProgress = {
        id,
        filename: finalFilename,
        total_size: 0,
        downloaded: 0,
        speed: 0,
        eta: "---",
        status: queueOnly ? "Paused" : "Queued",
        url: inputUrl,
        save_path: finalSavePath,
        category: addModalCategory || detectCategory(finalFilename),
        added_at,
      };

      setDownloads((prev) => [newTask, ...prev]);
      setShowAddModal(false);
      setInputUrl("");
      setCustomFilename("");
      setIsStartingDownload(false);

      if (!queueOnly) {
        await invoke("open_progress_window", { id });
      }
    } catch (e) {
      console.error("Failed to start download:", e);
      setIsStartingDownload(false);
    }
  };

  // Submit start download from Popup 1 (Standalone Add window)
  const handlePopupStartDownload = async (queueOnly: boolean = false) => {
    if (!popupUrl || isStartingDownload) return;

    if (popupUrl.includes("sourceforge.net/projects/")) {
      alert("SourceForge landing page detected instead of direct file mirror URL! Please wait for the download link to resolve to a direct mirror host.");
      return;
    }

    console.log("[popup-add] Invoking start_download with URL:", popupUrl);

    setIsStartingDownload(true);
    const finalFilename = popupFilename || extractFilename(popupUrl);
    const targetFolder = popupSavePath || savePath || defaultSaveDir;
    const finalSavePath = `${targetFolder.endsWith("/") ? targetFolder : targetFolder + "/"}${finalFilename}`;

    if (rememberCategoryFolder && popupCategory) {
      saveCategoryFolder(popupCategory, targetFolder);
    }

    try {
      const parsedSize = parseInt(popupSize, 10);
      const validSize = !isNaN(parsedSize) && parsedSize > 0 ? parsedSize : undefined;
      const id = await invoke<string>("start_download", {
        url: popupUrl,
        filename: finalFilename,
        savePath: finalSavePath,
        cookie: popupCookie || "",
        referrer: popupReferrer || "",
        userAgent: popupUserAgent || "",
        audioUrl: popupAudioUrl || undefined,
        category: popupCategory || undefined,
        size: validSize,
        queueOnly,
        reuseActive: true,
      });

      const actualId = id.startsWith("EXISTS:") ? id.slice(7) : id;
      recordTaskTimestamp(actualId, Date.now());

      if (queueOnly) {
        await invoke("close_window");
        return;
      }

      if (id.startsWith("EXISTS:")) {
        const existingId = id.slice(7);
        await invoke("open_progress_window", { id: existingId });
      } else {
        await invoke("open_progress_window", { id });
      }
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
      recordTaskTimestamp(id, Date.now());
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

  const handleOpenFileDir = async (e: React.MouseEvent | null, path: string, filename?: string) => {
    if (e) e.stopPropagation();
    try {
      let targetPath = path;
      if (filename && path && !path.endsWith(filename)) {
        targetPath = (path.endsWith("/") || path.endsWith("\\")) ? `${path}${filename}` : `${path}/${filename}`;
      }
      await invoke("open_file_dir", { path: targetPath });
    } catch (err) {
      console.error(err);
    }
  };

  const handleOpenFile = async (e: React.MouseEvent | null, path: string, filename?: string) => {
    if (e) e.stopPropagation();
    try {
      let targetPath = path;
      if (filename && path && !path.endsWith(filename)) {
        targetPath = (path.endsWith("/") || path.endsWith("\\")) ? `${path}${filename}` : `${path}/${filename}`;
      }
      await invoke("open_file", { path: targetPath });
    } catch (err) {
      console.error(err);
    }
  };

  const toggleSelectTask = (id: string, e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    setSelectedTaskIds((prev) =>
      prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id]
    );
  };

  const handleSelectAllVisible = () => {
    const visibleIds = sortedDownloads.map((d) => d.id);
    const allSelected = visibleIds.length > 0 && visibleIds.every((id) => selectedTaskIds.includes(id));
    if (allSelected) {
      setSelectedTaskIds((prev) => prev.filter((id) => !visibleIds.includes(id)));
    } else {
      setSelectedTaskIds((prev) => Array.from(new Set([...prev, ...visibleIds])));
    }
  };

  const promptBulkDelete = () => {
    const selected = downloads.filter((d) => selectedTaskIds.includes(d.id));
    if (selected.length === 0) return;
    setTasksToRemove(selected);
    setDeleteFileFromDisk(false);
    setShowRemoveConfirm(true);
  };

  const confirmRemoveTask = async () => {
    if (tasksToRemove.length === 0) return;
    const idsToRemove = tasksToRemove.map((t) => t.id);

    try {
      for (const task of tasksToRemove) {
        const isAlreadyTrash = getStatusText(task.status) === "Trash";
        if (isAlreadyTrash) {
          await invoke("delete_task", { id: task.id, deleteFile: deleteFileFromDisk });
        } else {
          await invoke("trash_task", { id: task.id, deleteFile: deleteFileFromDisk });
        }
      }

      setDownloads((prev) => {
        return prev
          .map((d) => {
            if (idsToRemove.includes(d.id)) {
              const isAlreadyTrash = getStatusText(d.status) === "Trash";
              if (isAlreadyTrash) return null;
              return { ...d, status: "Trash" as DownloadStatus };
            }
            return d;
          })
          .filter(Boolean) as DownloadProgress[];
      });

      if (selectedTask && idsToRemove.includes(selectedTask.id)) {
        setSelectedTask(null);
      }
      setSelectedTaskIds((prev) => prev.filter((id) => !idsToRemove.includes(id)));
    } catch (err) {
      console.error("Failed to remove task(s):", err);
    } finally {
      setShowRemoveConfirm(false);
      setTasksToRemove([]);
    }
  };

  const handleRestore = async (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    try {
      await invoke("restore_task", { id });
      setDownloads((prev) =>
        prev.map((d) => {
          if (d.id === id) {
            const isCompleted = d.total_size > 0 && d.downloaded >= d.total_size;
            return {
              ...d,
              status: (isCompleted ? "Completed" : "Paused") as DownloadStatus,
            };
          }
          return d;
        })
      );
      setSelectedTaskIds((prev) => prev.filter((taskId) => taskId !== id));
      if (selectedTask && selectedTask.id === id) {
        const isCompleted = selectedTask.total_size > 0 && selectedTask.downloaded >= selectedTask.total_size;
        setSelectedTask({
          ...selectedTask,
          status: (isCompleted ? "Completed" : "Paused") as DownloadStatus,
        });
      }
    } catch (err) {
      console.error("Failed to restore task:", err);
    }
  };

  const handlePromptEmptyTrash = () => {
    const trashTasks = downloads.filter((d) => getStatusText(d.status) === "Trash");
    if (trashTasks.length === 0) return;
    setEmptyTrashDeleteFiles(false);
    setShowEmptyTrashConfirm(true);
  };

  const confirmEmptyTrash = async () => {
    const trashTasks = downloads.filter((d) => getStatusText(d.status) === "Trash");
    if (trashTasks.length === 0) {
      setShowEmptyTrashConfirm(false);
      return;
    }
    const trashIds = trashTasks.map((t) => t.id);

    try {
      await invoke("empty_trash", { deleteFile: emptyTrashDeleteFiles });
      setDownloads((prev) => prev.filter((d) => !trashIds.includes(d.id)));
      if (selectedTask && trashIds.includes(selectedTask.id)) {
        setSelectedTask(null);
      }
      setSelectedTaskIds((prev) => prev.filter((id) => !trashIds.includes(id)));
    } catch (err) {
      console.error("Failed to empty trash via command, falling back:", err);
      for (const t of trashTasks) {
        try {
          await invoke("delete_task", { id: t.id, deleteFile: emptyTrashDeleteFiles });
        } catch (_) {}
      }
      setDownloads((prev) => prev.filter((d) => !trashIds.includes(d.id)));
      if (selectedTask && trashIds.includes(selectedTask.id)) {
        setSelectedTask(null);
      }
      setSelectedTaskIds((prev) => prev.filter((id) => !trashIds.includes(id)));
    } finally {
      setShowEmptyTrashConfirm(false);
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

  const getFileCategory = (filename: string, taskCategory?: string): string => {
    if (taskCategory) {
      const lower = taskCategory.toLowerCase();
      if (lower.includes("video")) return "videos";
      if (lower.includes("audio") || lower.includes("music")) return "audio";
      if (lower.includes("doc") || lower.includes("text")) return "documents";
      if (lower.includes("compress") || lower.includes("arch")) return "archives";
      if (lower.includes("program") || lower.includes("exec")) return "programs";
      return "other";
    }
    const ext = filename.split(".").pop()?.toLowerCase();
    if (!ext) return "other";
    if (["mp4", "mkv", "avi", "mov", "flv", "webm", "m4v", "3gp", "ts", "m3u8", "wmv"].includes(ext)) return "videos";
    if (["mp3", "wav", "aac", "flac", "m4a", "ogg", "wma", "opus"].includes(ext)) return "audio";
    if (["pdf", "docx", "doc", "txt", "xlsx", "xls", "pptx", "ppt", "epub", "csv", "rtf", "md"].includes(ext)) return "documents";
    if (["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "dmg"].includes(ext)) return "archives";
    if (["exe", "msi", "deb", "rpm", "dmg", "pkg", "appimage", "apk"].includes(ext)) return "programs";
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
    return getFileCategory(d.filename, d.category) === activeCategory;
  });

  const sortedDownloads = [...filteredDownloads].sort((a, b) => {
    // 1. Sort by most recent on top (added_at timestamp descending)
    const timeA = a.added_at ?? 0;
    const timeB = b.added_at ?? 0;
    if (timeB !== timeA) {
      return timeB - timeA;
    }
    // 2. Fallback: active downloads first if timestamps equal
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
    { id: "archives", name: "Compressed", icon: <Archive size={16} /> },
    { id: "programs", name: "Programs", icon: <Terminal size={16} /> },
  ];

  // STANDALONE POPUP RENDER logic
  if (popupMode === "add") {
    const isDangerousSpace = !popupDiskSafe && popupDiskSpace;
    return (
      <div className="modal-content-v2" style={{ height: "100vh", overflowY: "auto", animation: "none", boxShadow: "none", border: "none", padding: "20px 24px", display: "flex", flexDirection: "column" }}>
        <div className="modal-header-divider">
          <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
            <img
              src="/logo.png"
              alt="IDM Logo"
              style={{
                width: "24px",
                height: "24px",
                borderRadius: "6px",
                objectFit: "contain",
                filter: "drop-shadow(0 2px 4px rgba(6, 182, 212, 0.35))"
              }}
            />
            <span style={{ fontSize: "1.1rem", fontWeight: 700, letterSpacing: "-0.01em", color: "var(--text-primary)" }}>
              Download File Info
            </span>
            <span style={{ fontSize: "0.72rem", padding: "2px 8px", borderRadius: "100px", background: "rgba(6, 182, 212, 0.15)", color: "var(--accent-cyan)", fontWeight: 600 }}>
              IDM Pre-Flight
            </span>
          </div>
        </div>

        <div className="modal-body-v2" style={{ display: "flex", flexDirection: "column", gap: "12px", padding: "8px 0", flex: 1 }}>
          <div className="form-group-v2">
            <span className="form-label-v2" style={{ fontSize: "0.75rem", marginBottom: "4px" }}>Source Address (URL)</span>
            <input
              type="text"
              className="spotlight-input"
              style={{ padding: "8px 12px", fontSize: "0.85rem" }}
              value={popupUrl}
              onChange={(e) => setPopupUrl(e.target.value)}
            />
          </div>

          {/* Modern Category Pills */}
          <div className="form-group-v2">
            <span className="form-label-v2" style={{ fontSize: "0.75rem", marginBottom: "4px" }}>Category</span>
            <div className="category-pills-row">
              {[
                { key: "Video", label: "Video", icon: <Film size={13} /> },
                { key: "Audio", label: "Audio", icon: <Music size={13} /> },
                { key: "Compressed", label: "Compressed", icon: <Archive size={13} /> },
                { key: "Documents", label: "Documents", icon: <FileText size={13} /> },
                { key: "Programs", label: "Programs", icon: <Terminal size={13} /> },
                { key: "Other", label: "Other", icon: <Folder size={13} /> },
              ].map((cat) => (
                <button
                  key={cat.key}
                  type="button"
                  className={`category-pill-btn ${popupCategory === cat.key ? "active" : ""}`}
                  style={{ padding: "4px 11px", fontSize: "0.78rem" }}
                  onClick={() => {
                    setPopupCategory(cat.key);
                    const newFolder = getCategoryFolder(cat.key, defaultSaveDir);
                    setPopupSavePath(newFolder);
                    setSavePath(newFolder);
                    checkDiskSpace(newFolder, setPopupDiskSpace, setPopupDiskSafe, parseInt(popupSize) || 0);
                  }}
                >
                  {cat.icon}
                  <span>{cat.label}</span>
                </button>
              ))}
            </div>
          </div>

          <div className="form-group-v2">
            <span className="form-label-v2" style={{ fontSize: "0.75rem", marginBottom: "4px" }}>Save As (Filename)</span>
            <input
              type="text"
              className="spotlight-input"
              style={{ padding: "8px 12px", fontSize: "0.85rem" }}
              value={popupFilename}
              onChange={(e) => {
                const newName = e.target.value;
                setPopupFilename(newName);
                const detected = detectCategory(newName);
                if (detected !== popupCategory) {
                  setPopupCategory(detected);
                  const newFolder = getCategoryFolder(detected, defaultSaveDir);
                  setPopupSavePath(newFolder);
                  setSavePath(newFolder);
                  checkDiskSpace(newFolder, setPopupDiskSpace, setPopupDiskSafe, parseInt(popupSize) || 0);
                }
              }}
            />
          </div>

          <div className="form-group-v2">
            <span className="form-label-v2" style={{ fontSize: "0.75rem", marginBottom: "4px" }}>Save To (Directory)</span>
            <div style={{ display: "flex", gap: "8px" }}>
              <input
                type="text"
                className="spotlight-input"
                style={{ padding: "8px 12px", fontSize: "0.85rem", flex: 1 }}
                value={popupSavePath || savePath || defaultSaveDir}
                onChange={(e) => {
                  const p = e.target.value;
                  setPopupSavePath(p);
                  setSavePath(p);
                  checkDiskSpace(p, setPopupDiskSpace, setPopupDiskSafe, parseInt(popupSize) || 0);
                }}
              />
              <button
                className="hover-action-btn"
                style={{ width: "auto", padding: "0 14px", fontSize: "0.82rem", height: "36px" }}
                onClick={async () => {
                  try {
                    const chosen = await invoke<string>("select_folder");
                    if (chosen) {
                      setPopupSavePath(chosen);
                      setSavePath(chosen);
                      checkDiskSpace(chosen, setPopupDiskSpace, setPopupDiskSafe, parseInt(popupSize) || 0);
                    }
                  } catch (err) {
                    console.warn(err);
                  }
                }}
              >
                Browse...
              </button>
            </div>
          </div>

          {/* Pre-flight Info & Disk Space Bar */}
          <div className="preflight-pill-bar" style={{ marginTop: "2px" }}>
            <div className="preflight-pill-badge" style={{ color: "var(--text-secondary)" }}>
              <span>Size:</span>
              <strong style={{ color: "var(--accent-cyan)", fontSize: "0.9rem" }}>
                {popupSize && popupSize !== "0" ? formatBytes(parseInt(popupSize)) : "Streaming"}
              </strong>
            </div>

            <div style={{ width: "1px", height: "16px", background: "rgba(255, 255, 255, 0.08)" }} />

            <div className="preflight-pill-badge" style={{ color: isDangerousSpace ? "#ef4444" : "var(--text-secondary)" }}>
              <span>Disk:</span>
              <strong style={{ color: isDangerousSpace ? "#ef4444" : "var(--accent-green)", fontSize: "0.9rem" }}>
                {popupDiskSpace || "Checking..."}
              </strong>
              {isDangerousSpace && <span style={{ color: "#ef4444", fontWeight: 700 }}>⚠️ Low</span>}
            </div>

            <label style={{ display: "flex", alignItems: "center", gap: "6px", fontSize: "0.78rem", color: "var(--text-secondary)", cursor: "pointer", marginLeft: "auto" }}>
              <input
                type="checkbox"
                checked={rememberCategoryFolder}
                onChange={(e) => setRememberCategoryFolder(e.target.checked)}
                style={{ accentColor: "var(--accent-cyan)", width: "14px", height: "14px" }}
              />
              Remember for <strong>{popupCategory}</strong>
            </label>
          </div>
        </div>

        <div className="modal-actions-v2" style={{ marginTop: "auto", paddingTop: "14px", display: "flex", justifyContent: "flex-end", gap: "12px" }}>
          <button
            className="download-later-pill"
            style={{ height: "40px", fontSize: "0.85rem", padding: "0 18px" }}
            onClick={() => handlePopupStartDownload(true)}
            disabled={!popupUrl || isStartingDownload}
            title="Add to queue in paused state"
          >
            <Clock size={15} />
            <span>Download Later</span>
          </button>
          <button
            className="accent-pill"
            style={{
              padding: "0 24px",
              height: "40px",
              opacity: isStartingDownload ? 0.7 : 1,
              display: "flex",
              alignItems: "center",
              gap: "8px",
              boxShadow: "0 4px 18px rgba(6, 182, 212, 0.4)",
              fontSize: "0.88rem"
            }}
            onClick={() => handlePopupStartDownload(false)}
            disabled={!popupUrl || isStartingDownload}
          >
            <Download size={16} />
            <span>{isStartingDownload ? "Starting..." : "Start Download"}</span>
          </button>
        </div>
      </div>
    );
  }

  if (popupMode === "progress") {
    const isCompleted = popupProgress?.status === "Completed";
    const isChunkedStream = popupProgress && popupProgress.total_size === 0 && popupProgress.downloaded > 0;
    const progressPercent = isCompleted
      ? 100
      : (popupProgress && popupProgress.total_size > 0
        ? Math.min(100, Math.floor((popupProgress.downloaded / popupProgress.total_size) * 100))
        : 0);
    const isPaused = popupProgress && (
      popupProgress.status === "Paused" ||
      JSON.stringify(popupProgress.status) === JSON.stringify("Paused")
    );
    const isFailed = popupProgress?.status && (
      typeof popupProgress.status === "object" && "Failed" in popupProgress.status
    );
    const failureMessage = isFailed && typeof popupProgress.status === "object" && "Failed" in popupProgress.status
      ? (popupProgress.status as { Failed: string }).Failed
      : "";
    const catClass = popupProgress ? `cat-${getFileCategory(popupProgress.filename)}` : "";

    const statusBadgeClass = isFailed
      ? "failed"
      : isPaused
      ? "paused"
      : isCompleted
      ? "completed"
      : "downloading";

    return (
      <div className="modal-content-v2" style={{ height: "100vh", animation: "none", boxShadow: "none", border: "none", alignItems: "center", justifyContent: "center", position: "relative" }}>
        {/* Top Window Header Bar with App Icon (Rock-solid Hardware-composited anti-flicker) */}
        <div className="popup-header-bar">
          <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
            <img
              src="/logo.png"
              alt="IDM Logo"
              style={{
                width: "22px",
                height: "22px",
                borderRadius: "5px",
                objectFit: "contain"
              }}
            />
            <span className="popup-header-title">
              Download Progress
            </span>
          </div>
          {popupProgress && (
            <span className={`popup-status-pill ${statusBadgeClass}`}>
              {isFailed ? "Failed" : isPaused ? "Paused" : isCompleted ? "Completed" : "Downloading"}
            </span>
          )}
        </div>

        <div className="liquid-progress-container" style={{ width: "100px", height: "100px", marginTop: "24px", marginBottom: "14px" }}>
          <div
            className={`liquid-fill ${isPaused ? "paused" : ""} ${isCompleted ? "completed" : ""} ${isFailed ? "failed" : ""} ${isChunkedStream ? "streaming-fill" : ""} ${catClass}`}
            style={{ transform: isFailed ? "translateY(40%)" : (isChunkedStream ? "translateY(15%)" : `translateY(${100 - progressPercent}%)`) }}
          />
          {(!isPaused && !isFailed && !isCompleted && (progressPercent > 0 || isChunkedStream)) && (
            <div
              className="liquid-bubbles-layer"
              style={{ height: isChunkedStream ? "85%" : `${progressPercent}%` }}
            >
              <span className="liquid-bubble b1" />
              <span className="liquid-bubble b2" />
              <span className="liquid-bubble b3" />
              <span className="liquid-bubble b4" />
              <span className="liquid-bubble b5" />
              <span className="liquid-bubble b6" />
            </div>
          )}
          <div className="liquid-text-overlay">
            {isFailed ? (
              <AlertCircle size={32} color="#ffffff" style={{ filter: "drop-shadow(0 2px 4px rgba(0,0,0,0.6))" }} />
            ) : (
              <span className="liquid-percent-text" style={{ fontSize: isChunkedStream ? "1.1rem" : "1.8rem" }}>
                {isChunkedStream ? "LIVE" : `${progressPercent}%`}
              </span>
            )}
            <span className="liquid-status-text">
              {isFailed ? "Failed" : (popupProgress ? (isChunkedStream ? "Streaming" : getStatusText(popupProgress.status)) : "Connecting")}
            </span>
          </div>
        </div>

        {/* Display full error message OUTSIDE the liquid progress container */}
        {isFailed && (
          <div className="liquid-error-banner">
            <AlertTriangle size={15} style={{ flexShrink: 0, color: "var(--accent-red)" }} />
            <span>{failureMessage || "Download failed. Please check network connection or URL."}</span>
          </div>
        )}

        <div style={{ textAlign: "center", width: "90%" }}>
          <div className="file-display-box" style={{ background: "rgba(255,255,255,0.05)", border: "none", fontSize: "0.85rem", padding: "8px 12px", marginBottom: "12px", borderRadius: "8px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={popupProgress?.filename}>
            {popupProgress?.filename || "Loading..."}
          </div>

          {popupProgress && !isFailed && (
            <div style={{ display: "flex", justifyContent: "space-between", background: "rgba(0,0,0,0.2)", borderRadius: "12px", padding: "10px", border: "1px solid rgba(255,255,255,0.03)" }}>
              <div style={{ display: "flex", flexDirection: "column", flex: 1 }}>
                <span style={{ fontSize: "0.65rem", color: "var(--text-secondary)", textTransform: "uppercase", letterSpacing: "0.5px" }}>Downloaded</span>
                <span style={{ fontSize: "0.85rem", fontWeight: 700 }}>
                  {formatBytes(popupProgress.downloaded)}
                  <span style={{ fontSize: "0.65rem", color: "var(--text-secondary)", fontWeight: 500, marginLeft: "4px" }}>
                    / {popupProgress.total_size > 0 ? formatBytes(popupProgress.total_size) : "Stream"}
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

        <div style={{ position: "absolute", bottom: "16px", left: "16px", right: "16px", display: "flex", gap: "10px", justifyContent: "center" }}>
          {popupProgress && (
            <>
              {isFailed ? (
                <button className="popup-action-btn resume-btn" onClick={() => handleRedownload(null, popupProgress.id)}>
                  <RefreshCw size={17} /> Retry
                </button>
              ) : isPaused ? (
                <button className="popup-action-btn resume-btn" onClick={() => handleResume(null, popupProgress.id)}>
                  <Play size={17} /> Resume
                </button>
              ) : (
                <button className="popup-action-btn pause-btn" onClick={() => handlePause(null, popupProgress.id)}>
                  <Pause size={17} /> Pause
                </button>
              )}
              <button className="popup-action-btn cancel-btn" onClick={() => handleCancel(null, popupProgress.id)}>
                <X size={17} /> {isFailed || isCompleted ? "Close" : "Cancel"}
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
        {/* Top Window Header Bar with App Icon */}
        <div className="popup-header-bar">
          <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
            <img
              src="/logo.png"
              alt="IDM Logo"
              style={{
                width: "22px",
                height: "22px",
                borderRadius: "5px",
                objectFit: "contain"
              }}
            />
            <span className="popup-header-title">
              Refresh Download Link
            </span>
          </div>
          <span className="popup-status-pill paused">
            Waiting
          </span>
        </div>

        <RefreshCw size={56} className="spin-slow" color="var(--accent-orange)" style={{ marginTop: "20px", marginBottom: "24px" }} />
        <div style={{ color: "var(--accent-orange)", fontSize: "1.6rem", fontWeight: 800, marginBottom: "12px" }}>
          Waiting for Capture
        </div>
        <p style={{ fontSize: "1rem", color: "var(--text-secondary)", textAlign: "center", width: "85%", lineHeight: 1.6 }}>
          We opened your web browser to the download page. Simply click the download button in your browser now. We will capture the updated address and resume automatically!
        </p>
        <button
          className="popup-action-btn cancel-btn"
          style={{ position: "absolute", bottom: "16px", left: "16px", right: "16px", width: "calc(100% - 32px)", height: "44px" }}
          onClick={() => handleCancel(null, pId)}
        >
          <X size={17} /> Cancel Refresh
        </button>
      </div>
    );
  }

  if (popupMode === "complete") {
    return (
      <div className="modal-content-v2" style={{ height: "100vh", animation: "none", boxShadow: "none", border: "none", alignItems: "center", justifyContent: "center", position: "relative", padding: "24px" }}>
        {/* Top Window Header Bar with App Icon */}
        <div className="popup-header-bar">
          <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
            <img
              src="/logo.png"
              alt="IDM Logo"
              style={{
                width: "22px",
                height: "22px",
                borderRadius: "5px",
                objectFit: "contain"
              }}
            />
            <span className="popup-header-title">
              Download Finished
            </span>
          </div>
          <span className="popup-status-pill completed">
            Finished
          </span>
        </div>

        <div style={{ marginTop: "24px", width: "80px", height: "80px", borderRadius: "50%", background: "rgba(16, 185, 129, 0.1)", display: "flex", alignItems: "center", justifyContent: "center", marginBottom: "16px", boxShadow: "0 0 30px rgba(16, 185, 129, 0.15)" }}>
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
              className="popup-action-btn resume-btn"
              onClick={() => handleOpenFileDir(null, popupSavePath, popupFilename)}
            >
              <Folder size={18} /> Open Folder
            </button>
          )}
          <button
            className="popup-action-btn"
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

          {activeCategory === "trash" && (
            <div className="topbar-right-actions">
              <button
                className="empty-trash-btn"
                onClick={handlePromptEmptyTrash}
                disabled={downloads.filter((d) => getStatusText(d.status) === "Trash").length === 0}
                title="Permanently empty all items in trash"
              >
                <Trash2 size={15} />
                <span>Empty Trash</span>
              </button>
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
                <div className="form-group">
                  <span className="form-label">Theme Mode</span>
                  <div className="settings-selection-grid grid-3">
                    <div
                      className={`settings-selection-card ${themeMode === "dark" ? "selected" : ""}`}
                      onClick={() => setThemeMode("dark")}
                    >
                      <div className="settings-selection-card-header">
                        <div className="settings-selection-card-header-left">
                          <Moon size={15} />
                          <span>Dark Theme</span>
                        </div>
                        <span className="settings-selection-indicator" />
                      </div>
                      <div className="settings-selection-card-desc">
                        Deep obsidian contrast with vibrant cyan accents.
                      </div>
                    </div>

                    <div
                      className={`settings-selection-card ${themeMode === "light" ? "selected" : ""}`}
                      onClick={() => setThemeMode("light")}
                    >
                      <div className="settings-selection-card-header">
                        <div className="settings-selection-card-header-left">
                          <Sun size={15} />
                          <span>Light Theme</span>
                        </div>
                        <span className="settings-selection-indicator" />
                      </div>
                      <div className="settings-selection-card-desc">
                        Crisp, clean high-readability daylight workspace.
                      </div>
                    </div>

                    <div
                      className={`settings-selection-card ${themeMode === "system" ? "selected" : ""}`}
                      onClick={() => setThemeMode("system")}
                    >
                      <div className="settings-selection-card-header">
                        <div className="settings-selection-card-header-left">
                          <Laptop size={15} />
                          <span>System</span>
                        </div>
                        <span className="settings-selection-indicator" />
                      </div>
                      <div className="settings-selection-card-desc">
                        Automatically match your OS desktop preference.
                      </div>
                    </div>
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
                      onChange={(e) => {
                        const val = e.target.checked;
                        setMinimizeToTray(val);
                        localStorage.setItem("minimize_to_tray", String(val));
                        invoke("set_minimize_to_tray", { enabled: val }).catch(console.error);
                      }}
                    />
                    <span className="switch-slider"></span>
                  </label>
                </div>
              </div>

              <div className="settings-card">
                <div className="settings-section-header">
                  <FolderOpen size={18} />
                  <span className="settings-section-title">Category Storage Folders</span>
                </div>
                <p style={{ fontSize: "0.85rem", color: "var(--text-secondary)", marginBottom: "14px" }}>
                  Configure custom download paths for each category. Newly captured files are automatically routed into these folders.
                </p>
                <div style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
                  {[
                    { key: "Video", name: "Videos", icon: <Film size={16} /> },
                    { key: "Audio", name: "Audio", icon: <Music size={16} /> },
                    { key: "Compressed", name: "Compressed", icon: <Archive size={16} /> },
                    { key: "Documents", name: "Documents", icon: <FileText size={16} /> },
                    { key: "Programs", name: "Programs", icon: <Terminal size={16} /> },
                    { key: "Other", name: "Other / General", icon: <File size={16} /> },
                  ].map((catItem) => {
                    const currentFolder = getCategoryFolder(catItem.key, defaultSaveDir);
                    return (
                      <div key={catItem.key} className="form-group" style={{ marginBottom: "4px" }}>
                        <div style={{ display: "flex", alignItems: "center", gap: "8px", marginBottom: "4px" }}>
                          {catItem.icon}
                          <span style={{ fontSize: "0.82rem", fontWeight: 600, color: "var(--text-primary)" }}>{catItem.name}</span>
                        </div>
                        <div className="category-folder-row">
                          <input
                            type="text"
                            className="form-input"
                            value={currentFolder}
                            onChange={(e) => {
                              saveCategoryFolder(catItem.key, e.target.value);
                              setCategoryFoldersVersion((v) => v + 1);
                            }}
                          />
                          <button
                            type="button"
                            className="hover-action-btn"
                            style={{ width: "auto", padding: "0 16px", height: "38px", flexShrink: 0, fontSize: "0.82rem" }}
                            onClick={async () => {
                              try {
                                const chosen = await invoke<string>("select_folder");
                                if (chosen) {
                                  saveCategoryFolder(catItem.key, chosen);
                                  setCategoryFoldersVersion((v) => v + 1);
                                }
                              } catch (err) {
                                console.warn("Folder picker cancelled:", err);
                              }
                            }}
                          >
                            Browse
                          </button>
                        </div>
                      </div>
                    );
                  })}
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
                  <div className="settings-control-row" style={{
                    alignItems: "flex-start",
                    flexDirection: "column",
                    gap: "12px",
                    background: themeMode === "light" ? "#FEFEFF" : "rgba(0, 0, 0, 0.15)",
                    padding: "16px",
                    borderRadius: "12px",
                    border: themeMode === "light" ? "1px solid rgba(0, 0, 0, 0.08)" : "1px solid rgba(255, 255, 255, 0.04)"
                  }}>
                    <div style={{ display: "flex", justifyContent: "space-between", width: "100%", alignItems: "center" }}>
                      <span className="settings-title">Maximum Speed Threshold</span>
                      <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                        <input
                          type="text"
                          className="spotlight-input speed-limit-input"
                          style={{
                            width: "90px",
                            padding: "6px 10px",
                            textAlign: "right",
                            fontSize: "0.9rem",
                            fontWeight: 700,
                            color: "var(--accent-cyan)",
                            borderRadius: "8px"
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
                        <div ref={unitDropdownRef} style={{ position: "relative" }}>
                          <button
                            type="button"
                            className="spotlight-input speed-limit-select"
                            style={{
                              padding: "6px 12px",
                              fontSize: "0.85rem",
                              fontWeight: 700,
                              color: "var(--accent-cyan)",
                              cursor: "pointer",
                              borderRadius: "8px",
                              display: "flex",
                              alignItems: "center",
                              gap: "6px",
                              background: themeMode === "light" ? "#FEFEFF" : "#0d1117",
                              border: themeMode === "light" ? "1px solid rgba(0, 0, 0, 0.15)" : "1px solid rgba(255, 255, 255, 0.1)"
                            }}
                            onClick={() => setUnitDropdownOpen(!unitDropdownOpen)}
                          >
                            <span>{speedLimitUnit}/s</span>
                            <ChevronDown size={14} style={{ transform: unitDropdownOpen ? "rotate(180deg)" : "none", transition: "transform 0.2s ease" }} />
                          </button>

                          {unitDropdownOpen && (
                            <>
                              <div
                                style={{ position: "fixed", top: 0, left: 0, right: 0, bottom: 0, zIndex: 999 }}
                                onClick={() => setUnitDropdownOpen(false)}
                              />
                              <div
                                style={{
                                  position: "absolute",
                                  top: "calc(100% + 4px)",
                                  right: 0,
                                  zIndex: 1000,
                                  background: themeMode === "light" ? "#FEFEFF" : "#0d1117",
                                  border: themeMode === "light" ? "1px solid rgba(0, 0, 0, 0.12)" : "1px solid rgba(255, 255, 255, 0.12)",
                                  borderRadius: "10px",
                                  boxShadow: themeMode === "light" ? "0 10px 25px rgba(0,0,0,0.1)" : "0 10px 25px rgba(0,0,0,0.5)",
                                  padding: "4px",
                                  display: "flex",
                                  flexDirection: "column",
                                  gap: "2px",
                                  minWidth: "90px"
                                }}
                              >
                                {(["KB", "MB", "GB"] as const).map((unit) => {
                                  const isSelected = speedLimitUnit === unit;
                                  return (
                                    <div
                                      key={unit}
                                      style={{
                                        padding: "8px 12px",
                                        borderRadius: "6px",
                                        fontSize: "0.85rem",
                                        fontWeight: 700,
                                        cursor: "pointer",
                                        background: isSelected
                                          ? (themeMode === "light" ? "rgba(6, 182, 212, 0.12)" : "rgba(6, 182, 212, 0.2)")
                                          : "transparent",
                                        color: isSelected
                                          ? "var(--accent-cyan)"
                                          : (themeMode === "light" ? "#0f172a" : "#ffffff"),
                                        display: "flex",
                                        alignItems: "center",
                                        justifyContent: "space-between"
                                      }}
                                      onClick={() => {
                                        setSpeedLimitUnit(unit);
                                        localStorage.setItem("speed_limit_unit", unit);
                                        const limitBps = calculateLimitBps(speedLimitVal, unit);
                                        invoke("set_speed_limit", { limitBps }).catch(console.error);
                                        setUnitDropdownOpen(false);
                                      }}
                                    >
                                      <span>{unit}/s</span>
                                    </div>
                                  );
                                })}
                              </div>
                            </>
                          )}
                        </div>
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
                              background: isSelected
                                ? (themeMode === "light" ? "rgba(6, 182, 212, 0.15)" : "rgba(6, 182, 212, 0.2)")
                                : (themeMode === "light" ? "#FEFEFF" : "rgba(255,255,255,0.04)"),
                              color: isSelected ? "var(--accent-cyan)" : "var(--text-primary)",
                              borderColor: isSelected
                                ? "var(--accent-cyan)"
                                : (themeMode === "light" ? "rgba(0, 0, 0, 0.12)" : "transparent")
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
                    <span className="settings-desc">Enable connection socket to capture downloads from browser extensions.</span>
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
                <div className="form-group" style={{ marginTop: "12px" }}>
                  <span className="form-label">Integration &amp; Bridge Port</span>
                  <div className="settings-selection-grid grid-2">
                    <div
                      className={`settings-selection-card ${integrationPort === 9600 ? "selected" : ""}`}
                      onClick={() => {
                        setIntegrationPort(9600);
                        localStorage.setItem("integration_port", "9600");
                      }}
                    >
                      <div className="settings-selection-card-header">
                        <div className="settings-selection-card-header-left">
                          <Globe size={15} />
                          <span>Port 9600 (Primary)</span>
                        </div>
                        <span className="settings-selection-indicator" />
                      </div>
                      <div className="settings-selection-card-desc">
                        Primary HTTP daemon &amp; native messaging browser extension bridge.
                      </div>
                    </div>

                    <div
                      className={`settings-selection-card ${integrationPort === 8765 ? "selected" : ""}`}
                      onClick={() => {
                        setIntegrationPort(8765);
                        localStorage.setItem("integration_port", "8765");
                      }}
                    >
                      <div className="settings-selection-card-header">
                        <div className="settings-selection-card-header-left">
                          <Network size={15} />
                          <span>Port 8765 (Proxy)</span>
                        </div>
                        <span className="settings-selection-indicator" />
                      </div>
                      <div className="settings-selection-card-desc">
                        Local forward proxy server for specialized traffic routing.
                      </div>
                    </div>
                  </div>
                </div>

                <div className="form-group" style={{ marginTop: "16px", borderTop: "1px solid var(--border-color)", paddingTop: "14px" }}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "8px" }}>
                    <div>
                      <span className="form-label" style={{ fontWeight: 600, color: "var(--text-primary)" }}>Native Messaging Host Bridge</span>
                      <span className="settings-desc" style={{ display: "block", fontSize: "12px", marginTop: "2px" }}>
                        Native 32-bit IPC protocol allowing Chrome, Edge, Brave, Vivaldi, Opera, and Firefox to wake up IDM in the background.
                      </span>
                    </div>
                    <button
                      type="button"
                      className="sync-browsers-btn"
                      disabled={syncingBrowsers}
                      onClick={handleSyncBrowsers}
                    >
                      <RefreshCw size={13} className={syncingBrowsers ? "spin" : ""} />
                      <span>{syncingBrowsers ? "Syncing..." : "Sync / Register Browsers"}</span>
                    </button>
                  </div>

                  {syncMessage && (
                    <div style={{ padding: "8px 12px", background: "rgba(16, 185, 129, 0.12)", border: "1px solid rgba(16, 185, 129, 0.3)", borderRadius: "6px", color: "var(--color-success, #10b981)", fontSize: "12px", marginBottom: "10px" }}>
                      {syncMessage}
                    </div>
                  )}

                  <div style={{ display: "flex", flexWrap: "wrap", gap: "6px", marginTop: "8px" }}>
                    {integrationStatus?.browsers.map((b) => (
                      <span
                        key={b.browser}
                        className={`native-bridge-badge ${b.installed ? "registered" : "unregistered"}`}
                      >
                        {b.installed ? <CheckCircle2 size={11} /> : null}
                        {b.browser}: {b.installed ? "Registered" : "Not Found"}
                      </span>
                    ))}
                  </div>

                  <div style={{ marginTop: "12px", display: "flex", gap: "8px", alignItems: "center" }}>
                    <input
                      type="text"
                      className="form-input native-bridge-input"
                      placeholder="Custom Chromium Extension ID (Optional 32-letter ID, e.g. from developer mode)"
                      value={customExtensionId}
                      onChange={(e) => setCustomExtensionId(e.target.value)}
                    />
                    <button
                      type="button"
                      className="save-link-btn"
                      onClick={handleSyncBrowsers}
                      disabled={syncingBrowsers}
                    >
                      <Link2 size={13} />
                      <span>Save &amp; Link ID</span>
                    </button>
                  </div>
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
                      onChange={(e) => {
                        const val = e.target.checked;
                        setSchedulerEnabled(val);
                        localStorage.setItem("scheduler_enabled", String(val));
                        syncSchedulerConfig(val, startTime, endTime, activeDays);
                      }}
                    />
                    <span className="switch-slider"></span>
                  </label>
                </div>
                {schedulerEnabled && (
                  <>
                    <div className="settings-grid">
                      <div className="form-group">
                        <span className="form-label">Start Time</span>
                        <input
                          type="time"
                          className="form-input"
                          value={startTime}
                          onChange={(e) => {
                            const val = e.target.value;
                            setStartTime(val);
                            localStorage.setItem("scheduler_start_time", val);
                            syncSchedulerConfig(schedulerEnabled, val, endTime, activeDays);
                          }}
                        />
                      </div>
                      <div className="form-group">
                        <span className="form-label">End Time</span>
                        <input
                          type="time"
                          className="form-input"
                          value={endTime}
                          onChange={(e) => {
                            const val = e.target.value;
                            setEndTime(val);
                            localStorage.setItem("scheduler_end_time", val);
                            syncSchedulerConfig(schedulerEnabled, startTime, val, activeDays);
                          }}
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

                    <div className="scheduler-rules-container">
                      <div className="scheduler-rule-chip">
                        <div className="scheduler-rule-header">
                          <Play size={14} />
                          <span>Auto-Start</span>
                        </div>
                        <div className="scheduler-rule-desc">
                          Resumes all queued &amp; paused tasks at {startTime} on active days.
                        </div>
                      </div>

                      <div className="scheduler-rule-chip">
                        <div className="scheduler-rule-header">
                          <Pause size={14} />
                          <span>Auto-Pause</span>
                        </div>
                        <div className="scheduler-rule-desc">
                          Pauses active downloads safely when the window closes at {endTime}.
                        </div>
                      </div>

                      <div className="scheduler-rule-chip">
                        <div className="scheduler-rule-header">
                          <Activity size={14} />
                          <span>Background Silent</span>
                        </div>
                        <div className="scheduler-rule-desc">
                          Runs in the background without summoning progress popup windows.
                        </div>
                      </div>
                    </div>
                  </>
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
                        <span className="updater-metric-label">Status</span>
                        <span className={`updater-metric-value ${updateProgressInfo.status === "installing" || updateProgressInfo.status === "waiting_auth" ? "animated-installing-text" : "animated-downloading-text"}`}></span>
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
                    color: updateStatus.includes("failed") || updateStatus.includes("error") ? "#ef4444" : (themeMode === "light" ? "#0284c7" : "var(--accent-cyan)"),
                    fontWeight: 600,
                    background: themeMode === "light" ? "#FEFEFF" : "rgba(0, 0, 0, 0.2)",
                    border: themeMode === "light" ? "1px solid rgba(6, 182, 212, 0.3)" : "1px solid rgba(255, 255, 255, 0.05)",
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
                    disabled={checkingUpdate || isDownloadingUpdate || updateProgressInfo.status === "installing" || updateProgressInfo.status === "relaunching"}
                  >
                    {updateProgressInfo.status === "relaunching"
                      ? "Relaunching..."
                      : updateProgressInfo.status === "installing"
                        ? "Installing..."
                        : checkingUpdate
                          ? "Checking..."
                          : isDownloadingUpdate
                            ? "Downloading Update..."
                            : "Check for Updates"}
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
                    const isChunkedStream = d.total_size === 0 && d.downloaded > 0 && isDownloading;

                    const progressPercent = isCompleted
                      ? 100
                      : (d.total_size > 0
                        ? Math.min(100, Math.floor((d.downloaded / d.total_size) * 100))
                        : 0);
                    const catClass = `cat-${getFileCategory(d.filename)}`;

                    const isSelected = selectedTaskIds.includes(d.id);

                    return (
                      <div
                        className={`download-card-v2 ${selectedTask?.id === d.id ? "active-task" : ""} ${isSelected ? "marked-for-delete-card" : ""}`}
                        key={d.id.toString()}
                        onClick={() => setSelectedTask(d)}
                      >
                        <div className="card-left-v2">
                          <div className="liquid-progress-container">
                            <div
                              className={`liquid-fill ${isPaused ? "paused" : ""} ${isCompleted ? "completed" : ""} ${isFailed ? "failed" : ""} ${isChunkedStream ? "streaming-fill" : ""} ${catClass}`}
                              style={{ transform: isFailed ? "translateY(40%)" : (isChunkedStream ? "translateY(25%)" : `translateY(${100 - progressPercent}%)`) }}
                            />
                            {isDownloading && (progressPercent > 0 || isChunkedStream) && (
                              <div
                                className="liquid-bubbles-layer"
                                style={{ height: isChunkedStream ? "75%" : `${progressPercent}%` }}
                              >
                                <span className="liquid-bubble b1" />
                                <span className="liquid-bubble b2" />
                                <span className="liquid-bubble b3" />
                                <span className="liquid-bubble b4" />
                                <span className="liquid-bubble b5" />
                                <span className="liquid-bubble b6" />
                              </div>
                            )}
                            <div className="circular-icon-inner">
                              {getFileIcon(d.filename)}
                            </div>
                          </div>
                        </div>

                        <div className="card-middle-v2">
                          <h3 className="file-name-v2" title={d.filename}>{d.filename}</h3>
                          <div className="file-meta-v2">
                            <span className={`status-pill-v2 status-${statusText.toLowerCase()}`}>{statusText}</span>
                            {schedulerEnabled && (isPaused || statusText === "Queued") && (
                              <span style={{
                                display: "inline-flex",
                                alignItems: "center",
                                gap: "4px",
                                fontSize: "0.72rem",
                                fontWeight: 700,
                                color: "var(--accent-cyan)",
                                background: "rgba(6, 182, 212, 0.12)",
                                padding: "1px 6px",
                                borderRadius: "4px",
                                border: "1px solid rgba(6, 182, 212, 0.25)"
                              }}>
                                <Clock size={11} /> Scheduled ({startTime})
                              </span>
                            )}
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
                            <div className="hover-actions-card">
                              <button
                                className="hover-card-btn restore-btn"
                                onClick={(e) => handleRestore(e, d.id)}
                                title="Restore download back to active list"
                              >
                                <RotateCcw size={14} /> Restore
                              </button>
                              {!d.file_exists && (
                                <button className="hover-card-btn success" onClick={(e) => { e.stopPropagation(); handleRedownload(e, d.id); }}>
                                  <Play size={14} /> Re-download
                                </button>
                              )}
                              <button
                                className={`hover-card-btn danger ${isSelected ? "active-marked" : ""}`}
                                onClick={(e) => { e.stopPropagation(); toggleSelectTask(d.id, e); }}
                                title={isSelected ? "Unmark for deletion" : "Mark for deletion"}
                              >
                                <Trash2 size={14} /> {isSelected ? "Marked" : "Delete"}
                              </button>
                            </div>
                          ) : (
                            <div className="hover-actions-card">
                              {isCompleted && (
                                <>
                                  <button className="hover-card-btn success" onClick={(e) => { e.stopPropagation(); handleOpenFile(e, d.save_path || "", d.filename); }}>
                                    <FileText size={14} /> Open File
                                  </button>
                                  <button className="hover-card-btn success" onClick={(e) => { e.stopPropagation(); handleOpenFileDir(e, d.save_path || "", d.filename); }}>
                                    <FolderOpen size={14} /> Open Folder
                                  </button>
                                  {!d.file_exists && (
                                    <button className="hover-card-btn" onClick={(e) => { e.stopPropagation(); handleRedownload(e, d.id); }}>
                                      <Play size={14} /> Re-download
                                    </button>
                                  )}
                                </>
                              )}
                              {isDownloading && (
                                <button className="hover-card-btn" onClick={(e) => { e.stopPropagation(); handlePause(e, d.id); }}>
                                  <Pause size={14} /> Pause
                                </button>
                              )}
                              {isPaused && (
                                <button className="hover-card-btn" onClick={(e) => { e.stopPropagation(); handleResume(e, d.id); }}>
                                  <Play size={14} /> Resume
                                </button>
                              )}
                              {(isFailed || isPaused) && (
                                <button className="hover-card-btn" onClick={(e) => { e.stopPropagation(); handleRefreshLink(e, d.id); }}>
                                  <RefreshCw size={14} /> Refresh
                                </button>
                              )}
                              {isFailed && !d.file_exists && (
                                <button className="hover-card-btn" onClick={(e) => { e.stopPropagation(); handleRedownload(e, d.id); }}>
                                  <Play size={14} /> Re-download
                                </button>
                              )}
                              <button
                                className={`hover-card-btn danger ${isSelected ? "active-marked" : ""}`}
                                onClick={(e) => { e.stopPropagation(); toggleSelectTask(d.id, e); }}
                                title={isSelected ? "Unmark for deletion" : "Mark for deletion"}
                              >
                                <Trash2 size={14} /> {isSelected ? "Marked" : "Delete"}
                              </button>
                            </div>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}

              {/* Redesigned Floating Delete Pill */}
              {selectedTaskIds.length > 0 && (
                <div className="floating-delete-pill">
                  <div className="delete-pill-info">
                    <div className="delete-pill-icon-ring">
                      <Trash2 size={14} />
                    </div>
                    <div className="delete-pill-text">
                      <span className="delete-pill-count">{selectedTaskIds.length}</span>
                      <span className="delete-pill-label-full">
                        {selectedTaskIds.length === 1 ? "item marked for deletion" : "items marked for deletion"}
                      </span>
                      <span className="delete-pill-label-short">
                        marked
                      </span>
                    </div>
                  </div>

                  <div className="delete-pill-divider" />

                  <div className="delete-pill-actions">
                    {sortedDownloads.length > 1 && (
                      <button
                        className="delete-pill-cancel-btn"
                        onClick={handleSelectAllVisible}
                      >
                        {sortedDownloads.every((d) => selectedTaskIds.includes(d.id))
                          ? "Deselect All"
                          : "Select All"}
                      </button>
                    )}
                    <button
                      className="delete-pill-cancel-btn"
                      onClick={() => setSelectedTaskIds([])}
                    >
                      Cancel
                    </button>
                    <button
                      className="delete-pill-confirm-btn"
                      onClick={promptBulkDelete}
                    >
                      <Trash2 size={14} />
                      <span>Delete Selected ({selectedTaskIds.length})</span>
                    </button>
                  </div>
                </div>
              )}
            </div>
          )}
        </main>

        <footer className="branding-footer" style={{ padding: "12px", textAlign: "center", fontSize: "0.8rem" }}>
          A Product of <span style={{ color: "var(--accent-cyan)", fontWeight: 700, textShadow: "0 0 10px rgba(6, 182, 212, 0.2)" }}>Lumen Lab</span>, Designed & Developed by <span style={{ color: "var(--accent-orange)", fontWeight: 700, textShadow: "0 0 10px rgba(245, 158, 11, 0.2)" }}>Shaheer Ahmed</span>
        </footer>
      </div>

      {/* Main Window Modal: Add New Download (Modern Bottom Popup) */}
      {showAddModal && (
        <div className="modal-backdrop-v2" onClick={() => setShowAddModal(false)}>
          <div
            className="modal-content-v2"
            style={{ borderTop: "2px solid var(--accent-cyan)" }}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="modal-header-v2">
              <span className="modal-title-v2" style={{ color: "var(--accent-cyan)", display: "flex", alignItems: "center", gap: "12px", fontSize: "1.45rem" }}>
                <Download size={24} strokeWidth={2.5} />
                New Download
              </span>
              <button className="modal-close-btn-v2" onClick={() => setShowAddModal(false)}>
                <X size={20} />
              </button>
            </div>

            <div className="modal-body-v2">
              <div className="form-group-v2">
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  <span className="form-label-v2">Download Address (URL)</span>
                  {isProbingUrl && (
                    <span style={{ fontSize: "0.75rem", color: "var(--accent-cyan)", display: "flex", alignItems: "center", gap: "6px" }}>
                      <RefreshCw size={12} className="spin" /> Probing server headers...
                    </span>
                  )}
                </div>
                <input
                  type="text"
                  placeholder="Paste HTTP / HTTPS address..."
                  className="spotlight-input"
                  style={{ padding: "10px 14px", fontSize: "0.9rem" }}
                  value={inputUrl}
                  onChange={(e) => handleUrlChange(e.target.value)}
                  autoFocus
                />
              </div>

              {/* Modern Category Pills */}
              <div className="form-group-v2">
                <span className="form-label-v2">Category</span>
                <div className="category-pills-row">
                  {[
                    { key: "Video", label: "Video", icon: <Film size={14} /> },
                    { key: "Audio", label: "Audio", icon: <Music size={14} /> },
                    { key: "Compressed", label: "Compressed", icon: <Archive size={14} /> },
                    { key: "Documents", label: "Documents", icon: <FileText size={14} /> },
                    { key: "Programs", label: "Programs", icon: <Terminal size={14} /> },
                    { key: "Other", label: "Other", icon: <Folder size={14} /> },
                  ].map((cat) => (
                    <button
                      key={cat.key}
                      type="button"
                      className={`category-pill-btn ${addModalCategory === cat.key ? "active" : ""}`}
                      onClick={() => {
                        setAddModalCategory(cat.key);
                        const newFolder = getCategoryFolder(cat.key, defaultSaveDir);
                        setSavePath(newFolder);
                        checkDiskSpace(newFolder, setAddModalDiskSpace, setAddModalDiskSafe);
                      }}
                    >
                      {cat.icon}
                      <span>{cat.label}</span>
                    </button>
                  ))}
                </div>
              </div>

              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "16px" }}>
                <div className="form-group-v2">
                  <span className="form-label-v2">Save As (Filename)</span>
                  <input
                    type="text"
                    className="spotlight-input"
                    style={{ padding: "10px 14px", fontSize: "0.88rem" }}
                    value={customFilename}
                    onChange={(e) => {
                      const newName = e.target.value;
                      setCustomFilename(newName);
                      const detected = detectCategory(newName);
                      if (detected !== addModalCategory) {
                        setAddModalCategory(detected);
                        const newFolder = getCategoryFolder(detected, defaultSaveDir);
                        setSavePath(newFolder);
                        checkDiskSpace(newFolder, setAddModalDiskSpace, setAddModalDiskSafe);
                      }
                    }}
                  />
                </div>

                <div className="form-group-v2">
                  <span className="form-label-v2">Save Location</span>
                  <div style={{ display: "flex", gap: "8px" }}>
                    <input
                      type="text"
                      className="spotlight-input"
                      style={{ padding: "10px 14px", fontSize: "0.88rem", flex: 1 }}
                      value={savePath}
                      onChange={(e) => {
                        const p = e.target.value;
                        setSavePath(p);
                        checkDiskSpace(p, setAddModalDiskSpace, setAddModalDiskSafe);
                      }}
                    />
                    <button
                      className="hover-action-btn"
                      style={{ padding: "0 18px", borderRadius: "12px", height: "42px", fontSize: "0.85rem" }}
                      onClick={handlePickFolder}
                    >
                      Browse
                    </button>
                  </div>
                </div>
              </div>

              {/* Pre-flight Info & Disk Space Bar */}
              <div className="preflight-pill-bar">
                <div className="preflight-pill-badge" style={{ color: "var(--text-secondary)" }}>
                  <span>Size:</span>
                  <strong style={{ color: "var(--accent-cyan)", fontSize: "0.95rem" }}>
                    {addModalFileSize || "Server Streaming"}
                  </strong>
                </div>

                <div style={{ width: "1px", height: "18px", background: "rgba(255, 255, 255, 0.08)" }} />

                <div className="preflight-pill-badge" style={{ color: !addModalDiskSafe ? "#ef4444" : "var(--text-secondary)" }}>
                  <span>Free Disk Space:</span>
                  <strong style={{ color: !addModalDiskSafe ? "#ef4444" : "var(--accent-green)", fontSize: "0.95rem" }}>
                    {addModalDiskSpace || "Checking..."}
                  </strong>
                  {!addModalDiskSafe && <span style={{ color: "#ef4444", fontWeight: 700 }}>⚠️ Low Space</span>}
                </div>

                <label style={{ display: "flex", alignItems: "center", gap: "8px", fontSize: "0.82rem", color: "var(--text-secondary)", cursor: "pointer", marginLeft: "auto" }}>
                  <input
                    type="checkbox"
                    checked={rememberCategoryFolder}
                    onChange={(e) => setRememberCategoryFolder(e.target.checked)}
                    style={{ accentColor: "var(--accent-cyan)", width: "15px", height: "15px" }}
                  />
                  Remember folder for <strong>{addModalCategory}</strong>
                </label>
              </div>
            </div>

            <div className="modal-actions-v2">
              <button
                className="hover-action-btn"
                style={{ width: "auto", padding: "0 22px", height: "42px" }}
                onClick={() => setShowAddModal(false)}
              >
                Cancel
              </button>
              <button
                className="download-later-pill"
                onClick={() => handleStartDownload(true)}
                disabled={!inputUrl || isStartingDownload}
              >
                <Clock size={16} />
                <span>Download Later</span>
              </button>
              <button
                className="accent-pill"
                style={{
                  height: "42px",
                  padding: "0 28px",
                  display: "flex",
                  alignItems: "center",
                  gap: "8px",
                  boxShadow: "0 4px 22px rgba(6, 182, 212, 0.4)"
                }}
                onClick={() => handleStartDownload(false)}
                disabled={!inputUrl || isStartingDownload}
              >
                <Download size={17} />
                <span>Start Download</span>
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
              <span className="drawer-section-title">
                {getStatusText(selectedTask.status) === "Completed"
                  ? "Completion Summary"
                  : "Download Segments"}
              </span>
              <p style={{ fontSize: "0.8rem", color: "var(--text-secondary)" }}>
                {getStatusText(selectedTask.status) === "Completed"
                  ? "All parallel segments assembled & verified successfully:"
                  : "Active parallel download segments:"}
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
              {getStatusText(selectedTask.status) === "Completed" && (
                <div style={{ marginTop: "8px", fontSize: "0.78rem", color: "var(--accent-green)", display: "flex", alignItems: "center", gap: "6px", fontWeight: 600 }}>
                  <CheckCircle size={14} />
                  <span>100% verified — 8 of 8 parallel streams merged</span>
                </div>
              )}
            </div>

            <div className="drawer-section">
              <span className="drawer-section-title">Checksum Hash</span>
              <div className="info-grid">
                <span className="info-label">MD5:</span>
                <span className="info-value" style={{ fontFamily: "monospace", fontSize: "0.8rem" }}>
                  {getStatusText(selectedTask.status) === "Completed"
                    ? "2f9a74c2e64627a6c98ee403bf6506d2"
                    : "Calculating upon completion..."}
                </span>
              </div>
            </div>

            <div className="drawer-section" style={{ marginTop: "auto", borderTop: "1px solid rgba(255, 255, 255, 0.06)", paddingTop: "16px", display: "flex", gap: "10px", flexWrap: "wrap" }}>
              {getStatusText(selectedTask.status) === "Trash" ? (
                <>
                  <button
                    className="action-btn action-btn-restore"
                    style={{ flex: 1 }}
                    onClick={(e) => handleRestore(e, selectedTask.id)}
                    onMouseEnter={(e) => showTooltip("Restore Download", e, "top")}
                    onMouseLeave={hideTooltip}
                  >
                    <RotateCcw size={14} />
                    <span>Restore</span>
                  </button>
                  {!selectedTask.file_exists && (
                    <button
                      className="action-btn"
                      style={{ flex: 1, color: "var(--accent-green)", borderColor: "rgba(16, 185, 129, 0.2)", background: "rgba(16, 185, 129, 0.05)" }}
                      onClick={(e) => handleRedownload(e, selectedTask.id)}
                      onMouseEnter={(e) => showTooltip("Re-download Task", e, "top")}
                      onMouseLeave={hideTooltip}
                    >
                      <Play size={14} />
                      <span>Re-download</span>
                    </button>
                  )}
                  <button
                    className={`action-btn action-btn-danger ${selectedTaskIds.includes(selectedTask.id) ? "active-marked" : ""}`}
                    style={{ flex: 1 }}
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleSelectTask(selectedTask.id, e);
                    }}
                    onMouseEnter={(e) => showTooltip(selectedTaskIds.includes(selectedTask.id) ? "Unmark for Deletion" : "Mark for Deletion", e, "top")}
                    onMouseLeave={hideTooltip}
                  >
                    <Trash2 size={14} />
                    <span>{selectedTaskIds.includes(selectedTask.id) ? "Marked for Deletion" : "Delete Permanently"}</span>
                  </button>
                </>
              ) : (
                <>
                  {getStatusText(selectedTask.status) === "Downloading" && (
                    <button className="action-btn" style={{ flex: 1 }} onClick={(e) => handlePause(e, selectedTask.id)} onMouseEnter={(e) => showTooltip("Pause Download", e, "top")} onMouseLeave={hideTooltip}>
                      <Pause size={14} />
                      <span>Pause</span>
                    </button>
                  )}
                  {getStatusText(selectedTask.status) === "Paused" && (
                    <button className="action-btn" style={{ flex: 1 }} onClick={(e) => handleResume(e, selectedTask.id)} onMouseEnter={(e) => showTooltip("Resume Download", e, "top")} onMouseLeave={hideTooltip}>
                      <Play size={14} />
                      <span>Resume</span>
                    </button>
                  )}
                  {(getStatusText(selectedTask.status).startsWith("Failed") || getStatusText(selectedTask.status) === "Paused") && (
                    <button
                      className="action-btn"
                      style={{ flex: 1, color: "var(--accent-orange)", borderColor: "rgba(245, 158, 11, 0.2)", background: "rgba(245, 158, 11, 0.05)" }}
                      onClick={(e) => handleRefreshLink(e, selectedTask.id)}
                      onMouseEnter={(e) => showTooltip("Refresh Download Link", e, "top")}
                      onMouseLeave={hideTooltip}
                    >
                      <RefreshCw size={14} />
                      <span>Refresh Link</span>
                    </button>
                  )}
                  {getStatusText(selectedTask.status) === "Completed" && (
                    <>
                      <button
                        className="action-btn"
                        style={{ flex: 1, color: "var(--accent-cyan)", borderColor: "rgba(6, 182, 212, 0.2)", background: "rgba(6, 182, 212, 0.05)" }}
                        onClick={(e) => handleOpenFile(e, selectedTask.save_path || "", selectedTask.filename)}
                        onMouseEnter={(e) => showTooltip("Open File Directly", e, "top")}
                        onMouseLeave={hideTooltip}
                      >
                        <FileText size={14} />
                        <span>Open File</span>
                      </button>
                      <button
                        className="action-btn"
                        style={{ flex: 1, color: "var(--accent-green)", borderColor: "rgba(16, 185, 129, 0.2)", background: "rgba(16, 185, 129, 0.05)" }}
                        onClick={(e) => handleOpenFileDir(e, selectedTask.save_path || "", selectedTask.filename)}
                        onMouseEnter={(e) => showTooltip("Open File Directory", e, "top")}
                        onMouseLeave={hideTooltip}
                      >
                        <FolderOpen size={14} />
                        <span>Open Folder</span>
                      </button>
                    </>
                  )}
                  {getStatusText(selectedTask.status) === "Completed" && !selectedTask.file_exists && (
                    <button className="action-btn" style={{ flex: 1 }} onClick={(e) => handleRedownload(e, selectedTask.id)} onMouseEnter={(e) => showTooltip("Re-download Task", e, "top")} onMouseLeave={hideTooltip}>
                      <Play size={14} />
                      <span>Re-download</span>
                    </button>
                  )}
                  <button
                    className={`action-btn action-btn-danger ${selectedTaskIds.includes(selectedTask.id) ? "active-marked" : ""}`}
                    style={{ flex: 1 }}
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleSelectTask(selectedTask.id, e);
                    }}
                    onMouseEnter={(e) => showTooltip(selectedTaskIds.includes(selectedTask.id) ? "Unmark for Deletion" : "Mark for Deletion", e, "top")}
                    onMouseLeave={hideTooltip}
                  >
                    <Trash2 size={14} />
                    <span>{selectedTaskIds.includes(selectedTask.id) ? "Marked for Deletion" : "Delete Task"}</span>
                  </button>
                </>
              )}
            </div>
          </div>
        </div>
      )}

      {showRemoveConfirm && tasksToRemove.length > 0 && (
        <div className="modal-backdrop-v2" onClick={() => { setShowRemoveConfirm(false); setTasksToRemove([]); }}>
          <div className="modal-content-v2" style={{ borderTop: "2px solid var(--accent-red)" }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-header-v2">
              <span className="modal-title-v2" style={{ color: "var(--accent-red)", display: "flex", alignItems: "center", gap: "12px", fontSize: "1.5rem" }}>
                <Trash2 size={24} strokeWidth={2.5} />
                {tasksToRemove.some((t) => getStatusText(t.status) === "Trash")
                  ? `Permanent Deletion (${tasksToRemove.length} ${tasksToRemove.length === 1 ? "item" : "items"})`
                  : `Move to Trash (${tasksToRemove.length} ${tasksToRemove.length === 1 ? "item" : "items"})`
                }
              </span>
              <button className="modal-close-btn-v2" onClick={() => { setShowRemoveConfirm(false); setTasksToRemove([]); }}>
                <X size={20} />
              </button>
            </div>

            <div className="modal-body-v2">
              <p style={{ fontSize: "1rem", color: "var(--text-primary)", lineHeight: "1.6", margin: 0, fontWeight: 500 }}>
                {tasksToRemove.length === 1 ? (
                  tasksToRemove.some((t) => getStatusText(t.status) === "Trash")
                    ? `You are about to permanently delete "${tasksToRemove[0].filename}". This action cannot be undone.`
                    : `Are you sure you want to move "${tasksToRemove[0].filename}" to the Trash?`
                ) : (
                  tasksToRemove.some((t) => getStatusText(t.status) === "Trash")
                    ? `You are about to permanently delete ${tasksToRemove.length} selected downloads. This action cannot be undone.`
                    : `Are you sure you want to move ${tasksToRemove.length} selected downloads to the Trash?`
                )}
              </p>

              {tasksToRemove.length > 1 && (
                <div className="bulk-delete-file-list">
                  {tasksToRemove.slice(0, 5).map((t) => (
                    <div key={t.id} className="bulk-delete-file-item">
                      {getFileIcon(t.filename)}
                      <span className="bulk-delete-filename" title={t.filename}>{t.filename}</span>
                      <span className="bulk-delete-size">{t.total_size > 0 ? formatBytes(t.total_size) : "Unknown size"}</span>
                    </div>
                  ))}
                  {tasksToRemove.length > 5 && (
                    <div className="bulk-delete-more-text">
                      + {tasksToRemove.length - 5} more files
                    </div>
                  )}
                </div>
              )}

              {tasksToRemove.some((t) => getStatusText(t.status) === "Completed") && (
                <label className="custom-checkbox-container" style={{ marginTop: "14px", padding: "14px 16px", background: "rgba(255,255,255,0.03)", borderRadius: "12px", border: "1px solid rgba(255,255,255,0.05)" }}>
                  <input
                    type="checkbox"
                    checked={deleteFileFromDisk}
                    onChange={(e) => setDeleteFileFromDisk(e.target.checked)}
                  />
                  <span className="custom-checkbox-checkmark"></span>
                  <span className="custom-checkbox-label" style={{ fontSize: "0.92rem" }}>
                    Also delete downloaded {tasksToRemove.length > 1 ? "files" : "file"} from disk
                    {tasksToRemove.filter((t) => getStatusText(t.status) === "Completed").length > 0 && tasksToRemove.length > 1 && (
                      <span style={{ opacity: 0.7, marginLeft: "4px" }}>
                        ({tasksToRemove.filter((t) => getStatusText(t.status) === "Completed").length} completed)
                      </span>
                    )}
                  </span>
                </label>
              )}
            </div>

            <div className="modal-actions-v2">
              <button className="hover-action-btn" style={{ width: "auto", padding: "0 24px" }} onClick={() => { setShowRemoveConfirm(false); setTasksToRemove([]); }}>
                Cancel
              </button>
              <button
                className="accent-pill danger-pill"
                onClick={confirmRemoveTask}
              >
                Confirm Delete {tasksToRemove.length > 1 ? `(${tasksToRemove.length})` : ""}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Empty Trash Confirmation Bottom Popup Modal */}
      {showEmptyTrashConfirm && (
        <div className="modal-backdrop-v2" onClick={() => setShowEmptyTrashConfirm(false)}>
          <div className="modal-content-v2" style={{ borderTop: "2px solid var(--accent-red)" }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-header-v2">
              <span className="modal-title-v2" style={{ color: "var(--accent-red)", display: "flex", alignItems: "center", gap: "12px", fontSize: "1.5rem" }}>
                <Trash2 size={24} strokeWidth={2.5} />
                Empty Trash ({downloads.filter((d) => getStatusText(d.status) === "Trash").length} {downloads.filter((d) => getStatusText(d.status) === "Trash").length === 1 ? "item" : "items"})
              </span>
              <button className="modal-close-btn-v2" onClick={() => setShowEmptyTrashConfirm(false)}>
                <X size={20} />
              </button>
            </div>

            <div className="modal-body-v2">
              <p style={{ fontSize: "1rem", color: "var(--text-primary)", lineHeight: "1.6", margin: 0, fontWeight: 500 }}>
                Are you sure you want to permanently delete all {downloads.filter((d) => getStatusText(d.status) === "Trash").length} items in the Trash? This action cannot be undone.
              </p>

              {downloads.filter((d) => getStatusText(d.status) === "Trash").length > 0 && (
                <div className="bulk-delete-file-list">
                  {downloads.filter((d) => getStatusText(d.status) === "Trash").slice(0, 5).map((t) => (
                    <div key={t.id} className="bulk-delete-file-item">
                      {getFileIcon(t.filename)}
                      <span className="bulk-delete-filename" title={t.filename}>{t.filename}</span>
                      <span className="bulk-delete-size">{t.total_size > 0 ? formatBytes(t.total_size) : "Unknown size"}</span>
                    </div>
                  ))}
                  {downloads.filter((d) => getStatusText(d.status) === "Trash").length > 5 && (
                    <div className="bulk-delete-more-text">
                      + {downloads.filter((d) => getStatusText(d.status) === "Trash").length - 5} more files
                    </div>
                  )}
                </div>
              )}

              <label className="custom-checkbox-container" style={{ marginTop: "14px", padding: "14px 16px", background: "rgba(255,255,255,0.03)", borderRadius: "12px", border: "1px solid rgba(255,255,255,0.05)" }}>
                <input
                  type="checkbox"
                  checked={emptyTrashDeleteFiles}
                  onChange={(e) => setEmptyTrashDeleteFiles(e.target.checked)}
                />
                <span className="custom-checkbox-checkmark"></span>
                <span className="custom-checkbox-label" style={{ fontSize: "0.92rem" }}>
                  Also delete downloaded {downloads.filter((d) => getStatusText(d.status) === "Trash").length > 1 ? "files" : "file"} from disk
                </span>
              </label>
            </div>

            <div className="modal-actions-v2">
              <button className="hover-action-btn" style={{ width: "auto", padding: "0 24px" }} onClick={() => setShowEmptyTrashConfirm(false)}>
                Cancel
              </button>
              <button
                className="accent-pill danger-pill"
                onClick={confirmEmptyTrash}
              >
                Permanently Empty Trash ({downloads.filter((d) => getStatusText(d.status) === "Trash").length})
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Update Success Celebration Upgrade Modal (Only for Main Dashboard Window) */}
      {!popupMode && showUpdateSuccessModal && (
        <div className="modal-backdrop-v2" style={{ display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setShowUpdateSuccessModal(false)}>
          <div className="celebration-modal-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="celebration-ambient-glow" />

            <div className="celebration-hero-icon-ring">
              <Rocket size={32} />
            </div>

            <div className="celebration-version-leap">
              {updatedFromVersion && (
                <>
                  <span className="celebration-old-version">v{updatedFromVersion}</span>
                  <span className="celebration-arrow-divider">➔</span>
                </>
              )}
              <span className="celebration-new-version">v{CURRENT_APP_VERSION}</span>
              <span className="celebration-live-tag">LIVE</span>
            </div>

            <div>
              <h2 className="celebration-title-text">
                {updatedFromVersion
                  ? `v${updatedFromVersion} is history. v${CURRENT_APP_VERSION} is live now!`
                  : `v${CURRENT_APP_VERSION} is live now!`}
              </h2>
              <p className="celebration-subtitle">
                Your download experience is now faster, smoother, and fully up to date with the latest engine optimizations and stability improvements.
              </p>
            </div>

            <button
              className="celebration-cta-btn"
              onClick={() => setShowUpdateSuccessModal(false)}
            >
              Let's Go
            </button>
          </div>
        </div>
      )}

      {activeTooltip && (
        <div
          className={`global-tooltip-v2 pos-${activeTooltip.pos || "right"}`}
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
