import { useState, useEffect } from "react";
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
  FolderOpen
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
}

interface Category {
  id: string;
  name: string;
  icon: React.ReactNode;
}

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

  // Main Dashboard State
  const [activeCategory, setActiveCategory] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [downloads, setDownloads] = useState<DownloadProgress[]>([]);

  // Modal State (Manual Trigger)
  const [showAddModal, setShowAddModal] = useState(false);
  const [inputUrl, setInputUrl] = useState("");
  const [customFilename, setCustomFilename] = useState("");
  const [savePath, setSavePath] = useState("/home/shaheer/Downloads/");

  // Drawer State
  const [selectedTask, setSelectedTask] = useState<DownloadProgress | null>(null);

  // Settings State
  const [defaultSaveDir, setDefaultSaveDir] = useState("/home/shaheer/Downloads");
  const [autostart, setAutostart] = useState(true);
  const [minimizeToTray, setMinimizeToTray] = useState(true);
  const [maxChunks, setMaxChunks] = useState(8);
  const [speedLimitEnabled, setSpeedLimitEnabled] = useState(false);
  const [speedLimitKb, setSpeedLimitKb] = useState(2048);
  const [interceptDownloads, setInterceptDownloads] = useState(true);
  const [integrationPort, setIntegrationPort] = useState(9600);
  
  // Remove Task Modal States
  const [showRemoveConfirm, setShowRemoveConfirm] = useState(false);
  const [taskToRemove, setTaskToRemove] = useState<DownloadProgress | null>(null);
  const [deleteFileFromDisk, setDeleteFileFromDisk] = useState(false);

  // Scheduler State
  const [schedulerEnabled, setSchedulerEnabled] = useState(false);
  const [startTime, setStartTime] = useState("02:00");
  const [endTime, setEndTime] = useState("06:00");
  const [activeDays, setActiveDays] = useState<string[]>(["Mon", "Tue", "Wed", "Thu", "Fri"]);

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

  // Initialization query-param routing & initial loading
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const mode = params.get("popup");
    const url = params.get("url") || "";
    const filename = params.get("filename") || "";
    const savePath = params.get("save_path") || "";
    const cookie = params.get("cookie") || "";
    const referrer = params.get("referrer") || "";
    const taskId = params.get("id") || null;

    if (mode) {
      setPopupMode(mode);
      setPopupUrl(url);
      setPopupFilename(decodeURIComponent(filename));
      setPopupSavePath(decodeURIComponent(savePath));
      setPopupCookie(cookie);
      setPopupReferrer(referrer);
      setPopupTaskId(taskId);
      
      if (mode === "progress") {
        // Fetch progress state immediately from Rust backend to prevent Connecting... freeze
        if (taskId) {
          invoke<DownloadProgress | null>("get_download_progress", { id: taskId })
            .then((prog) => {
              if (prog) setPopupProgress(prog);
            })
            .catch(console.error);
        }
      } else if (mode === "complete") {
        setPopupFilename(params.get("filename") || "");
      }
    } else {
      // Main dashboard: fetch all active/completed downloads from backend
      invoke<DownloadProgress[]>("get_all_downloads")
        .then((list) => {
          if (list) setDownloads(list);
        })
        .catch(console.error);
    }
  }, []);

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

  // Dedicated polling loop for the progress popup window (bypasses all event/closure issues)
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
            // Trigger popup 3 and close this window when completed
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
        await new Promise<void>((res) => setTimeout(res, 500));
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
    if (!inputUrl) return;
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

      // Open standalone progress window for this task
      await invoke("open_progress_window", { id });
    } catch (e) {
      console.error("Failed to start download:", e);
    }
  };

  // Submit start download from Popup 1 (Standalone Add window)
  const handlePopupStartDownload = async () => {
    if (!popupUrl) return;
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
  const handlePause = async (e: React.MouseEvent | null, id: String) => {
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
      await invoke("redownload_task", { id });
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
        await getCurrentWindow().close();
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

  const handleRestore = async (e: React.MouseEvent | null, id: string) => {
    if (e) e.stopPropagation();
    try {
      await invoke("restore_task", { id });
      // Fetch updated status by polling or just let the progress listener sync it, 
      // but to be snappy we can update status to Paused or Completed locally
      setDownloads((prev) => 
        prev.map((d) => {
          if (d.id === id) {
            const isCompleted = d.total_size > 0 && d.downloaded >= d.total_size;
            return { ...d, status: isCompleted ? "Completed" : "Paused" };
          }
          return d;
        })
      );
    } catch (err) {
      console.error(err);
    }
  };


  const handleClosePopup = async () => {
    await getCurrentWindow().close();
  };

  // Helpers
  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return "0 Bytes";
    const k = 1024;
    const sizes = ["Bytes", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

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

  const getCount = (catId: string): number => {
    return downloads.filter((d) => {
      const statusText = getStatusText(d.status);
      if (catId === "trash") return statusText === "Trash";
      if (statusText === "Trash") return false; // Exclude from all other category counts
      
      if (catId === "all") return true;
      if (catId === "downloading") return statusText === "Downloading" || statusText === "Queued";
      if (catId === "completed") return statusText === "Completed";
      if (catId === "paused") return statusText === "Paused";
      return getFileCategory(d.filename) === catId;
    }).length;
  };

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
      <div className="standalone-popup">
        <div className="modal-header">
          <span className="modal-title">New Download Captured</span>
        </div>
        <div className="form-group">
          <span className="form-label">Source URL</span>
          <input type="text" className="form-input" value={popupUrl} onChange={(e) => setPopupUrl(e.target.value)} />
        </div>
        <div className="form-group">
          <span className="form-label">Save As Filename</span>
          <input type="text" className="form-input" value={popupFilename} onChange={(e) => setPopupFilename(e.target.value)} />
        </div>
        <div className="form-group">
          <span className="form-label">Save Folder Path</span>
          <div style={{ display: "flex", gap: "8px" }}>
            <input type="text" className="form-input" value={savePath} onChange={(e) => setSavePath(e.target.value)} />
            <button className="action-btn" style={{ padding: "0 14px", height: "40px" }} onClick={handlePickFolder}>Browse</button>
          </div>
        </div>
        <div className="modal-actions">
          <button className="action-btn" onClick={handleClosePopup}>Cancel</button>
          <button className="btn-add-download" style={{ padding: "8px 20px" }} onClick={handlePopupStartDownload} disabled={!popupUrl}>
            <span>Start Download</span>
          </button>
        </div>
      </div>
    );
  }

  if (popupMode === "progress") {
    const progressPercent = popupProgress && popupProgress.total_size > 0 
      ? Math.min(100, Math.floor((popupProgress.downloaded / popupProgress.total_size) * 100))
      : 0;
    const isPaused = popupProgress && (
      popupProgress.status === "Paused" ||
      JSON.stringify(popupProgress.status) === JSON.stringify("Paused")
    );

    return (
      <div className="standalone-popup standalone-popup--progress">
        <div className="popup-progress-header">
          <Activity size={16} color="var(--accent-cyan)" />
          <span>{popupProgress ? getStatusText(popupProgress.status) : "Connecting..."}</span>
        </div>

        <div className="popup-progress-body">
          {popupProgress ? (
            <>
              <div className="file-display-box" title={popupProgress.filename}>
                {popupProgress.filename}
              </div>

              <div className="popup-progress-bar-wrap">
                <div className="progress-bar-track">
                  <div 
                    className={`progress-bar-fill ${isPaused ? "paused" : ""}`}
                    style={{ width: `${progressPercent}%`, transition: "width 0.4s ease" }}
                  />
                </div>
                <div className="popup-progress-pct">{progressPercent}%</div>
              </div>

              <div style={{ display: "flex", justifyContent: "space-between", fontSize: "0.78rem", color: "var(--text-secondary)", marginTop: "-6px", padding: "0 2px" }}>
                <span>{formatBytes(popupProgress.downloaded)} of {popupProgress.total_size > 0 ? formatBytes(popupProgress.total_size) : "Dynamic Size"}</span>
                <span className={isPaused ? "accent-orange" : "accent-cyan"}>{getStatusText(popupProgress.status)}</span>
              </div>

              <div className="popup-progress-stats">
                <div className="popup-stat">
                  <span className="popup-stat-label">Speed</span>
                  <span className="popup-stat-value accent-cyan">{formatBytes(popupProgress.speed)}/s</span>
                </div>
                <div className="popup-stat">
                  <span className="popup-stat-label">ETA</span>
                  <span className="popup-stat-value">{popupProgress.eta}</span>
                </div>
              </div>
            </>
          ) : (
            <div className="popup-connecting">Connecting to backend...</div>
          )}
        </div>

        <div className="popup-progress-footer">
          {popupProgress && (
            <>
              {isPaused ? (
                <button className="action-btn" onClick={() => handleResume(null, popupProgress.id)}>
                  <Play size={14} />
                  <span>Resume</span>
                </button>
              ) : (
                <button className="action-btn" onClick={() => handlePause(null, popupProgress.id)}>
                  <Pause size={14} />
                  <span>Pause</span>
                </button>
              )}
              <button className="action-btn action-btn-danger" onClick={() => handleCancel(null, popupProgress.id)}>
                <X size={14} />
                <span>Cancel</span>
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
      <div className="standalone-popup">
        <div className="modal-header">
          <span className="modal-title" style={{ display: "flex", alignItems: "center", gap: "8px", color: "var(--accent-orange)" }}>
            <RefreshCw size={18} className="spin-slow" />
            Refreshing Link...
          </span>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: "12px", height: "100%", justifyContent: "center", alignItems: "center", textAlign: "center", padding: "10px 20px" }}>
          <div style={{ color: "var(--accent-orange)", fontSize: "1.1rem", fontWeight: 700, marginBottom: "4px" }}>
            Waiting for Capture
          </div>
          <p style={{ fontSize: "0.82rem", color: "var(--text-secondary)", margin: 0, lineHeight: 1.5 }}>
            We opened your web browser to the download page.
          </p>
          <p style={{ fontSize: "0.82rem", color: "var(--text-secondary)", margin: 0, lineHeight: 1.5 }}>
            Simply click the download button in your browser now. We will capture the updated address and resume downloading from your saved progress!
          </p>
          <div className="modal-actions" style={{ width: "100%", marginTop: "16px" }}>
            <button className="action-btn action-btn-danger" style={{ width: "100%" }} onClick={() => handleCancel(null, pId)}>
              <X size={14} />
              <span>Cancel Refresh</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (popupMode === "complete") {
    return (
      <div className="standalone-popup">
        <div className="modal-header">
          <span className="modal-title" style={{ display: "flex", alignItems: "center", gap: "8px", color: "var(--accent-green)" }}>
            <CheckCircle size={20} />
            Download Complete!
          </span>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: "10px", height: "100%" }}>
          <div className="file-display-box" style={{ background: "rgba(16, 185, 129, 0.03)", borderColor: "rgba(16, 185, 129, 0.15)" }}>
            {popupFilename}
          </div>
          <p style={{ fontSize: "0.8rem", color: "var(--text-secondary)", margin: "4px 0 0 0" }}>
            The file was downloaded successfully and saved to your destination directory.
          </p>
          <div className="modal-actions" style={{ gap: "10px" }}>
            {popupSavePath && (
              <button
                className="action-btn"
                style={{ flex: 1, justifyContent: "center", background: "rgba(16, 185, 129, 0.1)", borderColor: "rgba(16, 185, 129, 0.25)", color: "var(--accent-green)", padding: "9px 16px" }}
                onClick={() => handleOpenFileDir(null, popupSavePath)}
              >
                <FolderOpen size={15} />
                <span>Open Folder</span>
              </button>
            )}
            <button
              className="action-btn"
              style={{ flex: 1, justifyContent: "center", padding: "9px 16px" }}
              onClick={handleClosePopup}
            >
              <CheckCircle size={15} />
              <span>Dismiss</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  // DEFAULT MAIN DASHBOARD view
  return (
    <div className="app-shell">
      {/* Sidebar */}
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-icon">
            <Download size={18} color="#06070a" strokeWidth={3} />
          </div>
          <span className="brand-name">Impressive DM</span>
        </div>

        <nav className="sidebar-menu">
          {mainCategories.map((cat) => (
            <div
              key={cat.id}
              className={`menu-item ${activeCategory === cat.id ? "active" : ""}`}
              onClick={() => setActiveCategory(cat.id)}
            >
              {cat.icon}
              <span>{cat.name}</span>
              <span className="menu-item-count">{getCount(cat.id)}</span>
            </div>
          ))}

          <div style={{ height: "20px" }} />
          <div style={{ padding: "0 14px", fontSize: "0.75rem", textTransform: "uppercase", letterSpacing: "1px", color: "var(--text-muted)", fontWeight: 600, marginBottom: "8px" }}>
            File Types
          </div>

          {fileCategories.map((cat) => (
            <div
              key={cat.id}
              className={`menu-item ${activeCategory === cat.id ? "active" : ""}`}
              onClick={() => setActiveCategory(cat.id)}
            >
              {cat.icon}
              <span>{cat.name}</span>
              <span className="menu-item-count">{getCount(cat.id)}</span>
            </div>
          ))}
        </nav>

        <div className="sidebar-footer">
          <div className={`menu-item ${activeCategory === "settings" ? "active" : ""}`} onClick={() => setActiveCategory("settings")}>
            <Settings size={18} />
            <span>Settings</span>
          </div>
          <div className="sidebar-credit-footer">
            <div>A Product of <span className="credit-lumen">Lumen Lab</span></div>
            <div style={{ marginTop: "2px" }}>Designed & Developed by <span className="credit-shaheer">Shaheer Ahmed</span></div>
          </div>
        </div>
      </aside>

      {/* Main Container */}
      <div className="main-container">
        {/* Top Bar */}
        <header className="topbar">
          <div className="topbar-left">
            <div className="search-container">
              <Search size={18} className="search-icon" />
              <input
                type="text"
                placeholder="Search downloads..."
                className="search-input"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
            </div>
          </div>

          <div className="topbar-right">
            <button className="btn-add-download" onClick={handleOpenAddModal}>
              <Plus size={16} strokeWidth={3} />
              <span>Add Download</span>
            </button>
          </div>
        </header>

        {/* Content Area */}
        <main className="content-area">
          {activeCategory === "settings" ? (
            <div className="settings-container">
              <div className="settings-card">
                <div className="settings-section-header">
                  <Sliders size={18} />
                  <span className="settings-section-title">General Settings</span>
                </div>
                <div className="form-group">
                  <span className="form-label">Default Downloads Directory</span>
                  <input
                    type="text"
                    className="form-input"
                    value={defaultSaveDir}
                    onChange={(e) => setDefaultSaveDir(e.target.value)}
                  />
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
                      onChange={(e) => setAutostart(e.target.checked)}
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
                    onChange={(e) => setMaxChunks(parseInt(e.target.value))}
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
                      onChange={(e) => setSpeedLimitEnabled(e.target.checked)}
                    />
                    <span className="switch-slider"></span>
                  </label>
                </div>
                {speedLimitEnabled && (
                  <div className="settings-control-row" style={{ alignItems: "flex-start", flexDirection: "column", gap: "10px" }}>
                    <div className="settings-info-col">
                      <span className="settings-title">Speed Limit Threshold ({formatBytes(speedLimitKb * 1024)}/s)</span>
                    </div>
                    <input 
                      type="range" 
                      min="128" 
                      max="102400" 
                      step="128"
                      className="range-slider"
                      value={speedLimitKb}
                      onChange={(e) => setSpeedLimitKb(parseInt(e.target.value))}
                    />
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
                      onChange={(e) => setInterceptDownloads(e.target.checked)}
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
                  {filteredDownloads.map((d) => {
                    const statusText = getStatusText(d.status);
                    const isDownloading = statusText === "Downloading";
                    const isPaused = statusText === "Paused";
                    const isCompleted = statusText === "Completed";
                    const isFailed = statusText.startsWith("Failed");
                    const isTrash = statusText === "Trash";
                    
                    const progressPercent = d.total_size > 0 
                      ? Math.min(100, Math.floor((d.downloaded / d.total_size) * 100))
                      : 0;

                    return (
                      <div 
                        className="download-card" 
                        key={d.id.toString()}
                        onClick={() => setSelectedTask(d)}
                      >
                        <div className="card-header">
                          <div className="file-icon-container">
                            {getFileIcon(d.filename)}
                          </div>
                          <div className="file-info">
                            <span className="file-name" title={d.filename}>{d.filename}</span>
                            <div className="file-meta">
                              <span>{formatBytes(d.downloaded)} of {d.total_size > 0 ? formatBytes(d.total_size) : "Unknown size"}</span>
                              <span style={{ color: "var(--text-muted)" }}>•</span>
                              <span className={`status-badge status-${statusText.toLowerCase()}`}>
                                {statusText}
                              </span>
                            </div>
                          </div>
                        </div>

                        <div className="progress-container">
                          <div className="progress-bar-track">
                            <div 
                              className={`progress-bar-fill ${isPaused ? "paused" : ""} ${isCompleted ? "completed" : ""}`}
                              style={{ width: `${progressPercent}%` }}
                            />
                          </div>
                          <div className="progress-stats">
                            <span>{progressPercent}%</span>
                            <div className="speed-eta">
                              {isDownloading && (
                                <>
                                  <span>{formatBytes(d.speed)}/s</span>
                                  <span style={{ color: "var(--text-muted)" }}>•</span>
                                  <span>ETA: {d.eta}</span>
                                </>
                              )}
                              {isPaused && <span>Paused</span>}
                              {isCompleted && <span>Finished</span>}
                              {isFailed && <span style={{ color: "var(--accent-red)" }}>Error</span>}
                            </div>
                          </div>
                        </div>

                        <div className="card-actions">
                          {isTrash ? (
                            <>
                              <button 
                                className="action-btn" 
                                style={{ color: "var(--accent-green)", borderColor: "rgba(16, 185, 129, 0.2)", background: "rgba(16, 185, 129, 0.05)" }}
                                onClick={(e) => handleRestore(e, d.id)}
                              >
                                <RefreshCw size={14} />
                                <span>Restore</span>
                              </button>
                              <button className="action-btn action-btn-danger" onClick={(e) => promptRemoveTask(e, d)}>
                                <Trash2 size={14} />
                                <span>Delete Permanently</span>
                              </button>
                            </>
                          ) : (
                            <>
                              {isDownloading && (
                                <button className="action-btn" onClick={(e) => handlePause(e, d.id)}>
                                  <Pause size={14} />
                                  <span>Pause</span>
                                </button>
                              )}
                              {isPaused && (
                                <button className="action-btn" onClick={(e) => handleResume(e, d.id)}>
                                  <Play size={14} />
                                  <span>Resume</span>
                                </button>
                              )}
                              {(isFailed || isPaused) && (
                                <button 
                                  className="action-btn" 
                                  style={{ color: "var(--accent-orange)", borderColor: "rgba(245, 158, 11, 0.2)", background: "rgba(245, 158, 11, 0.05)" }} 
                                  onClick={(e) => handleRefreshLink(e, d.id)}
                                >
                                  <RefreshCw size={14} />
                                  <span>Refresh Link</span>
                                </button>
                              )}
                              {isFailed && (
                                <button className="action-btn" onClick={(e) => handleRedownload(e, d.id)}>
                                  <Play size={14} />
                                  <span>Re-download</span>
                                </button>
                              )}
                              {isCompleted && (
                                <button 
                                  className="action-btn" 
                                  style={{ color: "var(--accent-green)", borderColor: "rgba(16, 185, 129, 0.2)", background: "rgba(16, 185, 129, 0.05)" }}
                                  onClick={(e) => handleOpenFileDir(e, d.save_path || "")}
                                >
                                  <FolderOpen size={14} />
                                  <span>Open Folder</span>
                                </button>
                              )}
                              {isCompleted && (
                                <button className="action-btn" onClick={(e) => handleRedownload(e, d.id)}>
                                  <Play size={14} />
                                  <span>Re-download</span>
                                </button>
                              )}
                              <button className="action-btn action-btn-danger" onClick={(e) => promptRemoveTask(e, d)}>
                                <X size={14} />
                                <span>Remove</span>
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
      </div>

      {/* Main Window Modal: Add New Download (triggered manually) */}
      {showAddModal && (
        <div className="modal-backdrop" onClick={() => setShowAddModal(false)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <span className="modal-title">New Download</span>
              <button className="modal-close-btn" onClick={() => setShowAddModal(false)}>
                <X size={18} />
              </button>
            </div>

            <div className="form-group">
              <span className="form-label">Download URL</span>
              <input
                type="text"
                placeholder="Paste HTTP / HTTPS address..."
                className="form-input"
                value={inputUrl}
                onChange={(e) => handleUrlChange(e.target.value)}
              />
            </div>

            <div className="form-group">
              <span className="form-label">Save Filename</span>
              <input
                type="text"
                placeholder="Enter filename..."
                className="form-input"
                value={customFilename}
                onChange={(e) => setCustomFilename(e.target.value)}
              />
            </div>

            <div className="form-group">
              <span className="form-label">Save Location</span>
              <div style={{ display: "flex", gap: "8px" }}>
                <input
                  type="text"
                  placeholder="Enter absolute directory path..."
                  className="form-input"
                  value={savePath}
                  onChange={(e) => setSavePath(e.target.value)}
                />
                <button className="action-btn" style={{ padding: "0 14px", height: "46px" }} onClick={handlePickFolder}>Browse</button>
              </div>
            </div>

            <div className="modal-actions">
              <button className="action-btn" onClick={() => setShowAddModal(false)}>
                Cancel
              </button>
              <button 
                className="btn-add-download" 
                style={{ padding: "8px 20px" }}
                onClick={handleStartDownload}
                disabled={!inputUrl}
              >
                <span>Download Now</span>
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
                  <button 
                    className="action-btn" 
                    style={{ flex: 1, color: "var(--accent-green)", borderColor: "rgba(16, 185, 129, 0.2)", background: "rgba(16, 185, 129, 0.05)" }}
                    onClick={(e) => handleRestore(e, selectedTask.id)}
                  >
                    <RefreshCw size={14} />
                    <span>Restore</span>
                  </button>
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
                  {getStatusText(selectedTask.status) === "Completed" && (
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
        <div className="modal-backdrop">
          <div className="download-modal" style={{ maxWidth: "420px" }}>
            <div className="modal-header">
              <span className="modal-title" style={{ color: "var(--accent-red)", display: "flex", alignItems: "center", gap: "8px" }}>
                <Trash2 size={20} />
                {getStatusText(taskToRemove.status) === "Trash" ? "Permanent Delete" : "Move to Trash"}
              </span>
              <button className="modal-close-btn" onClick={() => { setShowRemoveConfirm(false); setTaskToRemove(null); }}>
                <X size={18} />
              </button>
            </div>
            
            <div style={{ display: "flex", flexDirection: "column", gap: "16px", marginTop: "8px" }}>
              <p style={{ fontSize: "0.85rem", color: "var(--text-primary)", lineHeight: "1.5", margin: 0 }}>
                {getStatusText(taskToRemove.status) === "Trash" 
                  ? `Are you sure you want to permanently delete "${taskToRemove.filename}"? This action cannot be undone.`
                  : `Are you sure you want to move "${taskToRemove.filename}" to Trash?`
                }
              </p>

              {getStatusText(taskToRemove.status) === "Completed" && (
                <label style={{ display: "flex", alignItems: "center", gap: "10px", cursor: "pointer", userSelect: "none", fontSize: "0.85rem", color: "var(--text-secondary)" }}>
                  <input 
                    type="checkbox" 
                    checked={deleteFileFromDisk} 
                    onChange={(e) => setDeleteFileFromDisk(e.target.checked)}
                    style={{ accentColor: "var(--accent-red)", width: "16px", height: "16px" }}
                  />
                  <span>Delete downloaded file from disk</span>
                </label>
              )}

              <div className="modal-actions" style={{ marginTop: "8px" }}>
                <button className="action-btn" style={{ flex: 1, justifyContent: "center" }} onClick={() => { setShowRemoveConfirm(false); setTaskToRemove(null); }}>
                  <span>Cancel</span>
                </button>
                <button 
                  className="btn-add-download" 
                  style={{ flex: 1, background: "var(--accent-red)", border: "none", boxShadow: "0 0 16px rgba(239, 68, 68, 0.2)", padding: "10px 16px" }} 
                  onClick={confirmRemoveTask}
                >
                  <span>Confirm</span>
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
