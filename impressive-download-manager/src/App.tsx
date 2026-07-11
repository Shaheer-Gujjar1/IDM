import { useState, useEffect } from "react";
import { 
  Download, 
  Search, 
  Plus, 
  TrendingUp, 
  Gauge, 
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
  File
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type DownloadStatus = "Queued" | "Downloading" | "Paused" | "Completed" | { Failed: string };

interface DownloadProgress {
  id: String;
  filename: string;
  total_size: number;
  downloaded: number;
  speed: number;
  eta: string;
  status: DownloadStatus;
}

interface Category {
  id: string;
  name: string;
  icon: React.ReactNode;
}

function App() {
  const [activeCategory, setActiveCategory] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [downloads, setDownloads] = useState<DownloadProgress[]>([
    {
      id: "mock-1",
      filename: "ubuntu-26.04-desktop-amd64.iso",
      total_size: 4724464640,
      downloaded: 2139062272,
      speed: 12478054,
      eta: "3m 27s",
      status: "Downloading"
    },
    {
      id: "mock-2",
      filename: "Big_Buck_Bunny_1080p.mp4",
      total_size: 276134980,
      downloaded: 276134980,
      speed: 0,
      eta: "0s",
      status: "Completed"
    },
    {
      id: "mock-3",
      filename: "rust-programming-guide.pdf",
      total_size: 14570182,
      downloaded: 4587520,
      speed: 0,
      eta: "---",
      status: "Paused"
    }
  ]);

  // Handle live progress updates from Tauri backend
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    async function setupListener() {
      unlisten = await listen<DownloadProgress>("download-progress", (event) => {
        setDownloads((prev) => {
          const index = prev.findIndex((d) => d.id === event.payload.id);
          if (index !== -1) {
            const updated = [...prev];
            updated[index] = event.payload;
            return updated;
          } else {
            return [event.payload, ...prev];
          }
        });
      });
    }

    setupListener().catch(console.error);

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Action Triggers
  const handlePause = async (id: String) => {
    if (id.startsWith("mock")) {
      setDownloads((prev) =>
        prev.map((d) => (d.id === id ? { ...d, status: "Paused", speed: 0, eta: "---" } : d))
      );
      return;
    }
    try {
      await invoke("pause_download", { id });
    } catch (e) {
      console.error("Pause failed:", e);
    }
  };

  const handleResume = async (id: String) => {
    if (id.startsWith("mock")) {
      setDownloads((prev) =>
        prev.map((d) => (d.id === id ? { ...d, status: "Downloading", speed: 8524102, eta: "5s" } : d))
      );
      return;
    }
    try {
      await invoke("resume_download", { id });
    } catch (e) {
      console.error("Resume failed:", e);
    }
  };

  const handleCancel = async (id: String) => {
    if (id.startsWith("mock")) {
      setDownloads((prev) => prev.filter((d) => d.id !== id));
      return;
    }
    try {
      await invoke("cancel_download", { id });
      setDownloads((prev) => prev.filter((d) => d.id !== id));
    } catch (e) {
      console.error("Cancel failed:", e);
    }
  };

  // Helper to format file sizes
  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return "0 Bytes";
    const k = 1024;
    const sizes = ["Bytes", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

  // Helper to resolve icon by extension
  const getFileIcon = (filename: string) => {
    const ext = filename.split(".").pop()?.toLowerCase();
    if (!ext) return <File size={22} />;
    
    if (["mp4", "mkv", "avi", "mov", "flv", "webm"].includes(ext)) return <Film size={22} />;
    if (["mp3", "wav", "aac", "flac", "m4a", "ogg"].includes(ext)) return <Music size={22} />;
    if (["pdf", "docx", "doc", "txt", "xlsx", "pptx", "epub"].includes(ext)) return <FileText size={22} />;
    if (["zip", "rar", "7z", "tar", "gz", "bz2"].includes(ext)) return <Archive size={22} />;
    
    return <File size={22} />;
  };

  // Categorize standard extensions
  const getFileCategory = (filename: string): string => {
    const ext = filename.split(".").pop()?.toLowerCase();
    if (!ext) return "other";
    if (["mp4", "mkv", "avi", "mov", "flv", "webm"].includes(ext)) return "videos";
    if (["mp3", "wav", "aac", "flac", "m4a", "ogg"].includes(ext)) return "audio";
    if (["pdf", "docx", "doc", "txt", "xlsx", "pptx", "epub"].includes(ext)) return "documents";
    if (["zip", "rar", "7z", "tar", "gz", "bz2"].includes(ext)) return "archives";
    return "other";
  };

  // Helper to resolve status string
  const getStatusText = (status: DownloadStatus): string => {
    if (typeof status === "string") return status;
    if (status && typeof status === "object" && "Failed" in status) {
      return `Failed: ${status.Failed}`;
    }
    return "Unknown";
  };

  // Filtering Logic
  const filteredDownloads = downloads.filter((d) => {
    // Search filter
    if (searchQuery && !d.filename.toLowerCase().includes(searchQuery.toLowerCase())) {
      return false;
    }

    // Category filter
    const statusText = getStatusText(d.status);
    if (activeCategory === "all") return true;
    if (activeCategory === "downloading") return statusText === "Downloading" || statusText === "Queued";
    if (activeCategory === "completed") return statusText === "Completed";
    if (activeCategory === "paused") return statusText === "Paused";
    if (activeCategory === "trash") return false; // In a full app, tracks deleted items

    // File type filters
    const cat = getFileCategory(d.filename);
    return cat === activeCategory;
  });

  // Calculate dynamic menu counters
  const getCount = (catId: string): number => {
    return downloads.filter((d) => {
      const statusText = getStatusText(d.status);
      if (catId === "all") return true;
      if (catId === "downloading") return statusText === "Downloading" || statusText === "Queued";
      if (catId === "completed") return statusText === "Completed";
      if (catId === "paused") return statusText === "Paused";
      if (catId === "trash") return 0;
      
      return getFileCategory(d.filename) === catId;
    }).length;
  };

  // Layout Categories
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

  // Calculate total speed of active downloads
  const totalSpeed = downloads
    .filter((d) => getStatusText(d.status) === "Downloading")
    .reduce((sum, d) => sum + d.speed, 0);

  const handleAddDownload = () => {
    // Phase 4: Will wire up the gorgeous slide overlay modal
    // For now, let's create a temporary mock download to show UI functionality
    const newMock: DownloadProgress = {
      id: `mock-${Date.now()}`,
      filename: `dynamic-captured-video-${Math.floor(Math.random() * 100)}.mp4`,
      total_size: 852410214,
      downloaded: 0,
      speed: 7852410,
      eta: "1m 48s",
      status: "Downloading"
    };
    setDownloads([newMock, ...downloads]);
  };

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
            <div className="system-stats">
              <div className="stat-item">
                <TrendingUp size={14} color="var(--accent-cyan)" />
                <span>Speed:</span>
                <span className="stat-value">{formatBytes(totalSpeed)}/s</span>
              </div>
              <div style={{ width: "1px", height: "14px", background: "var(--border-color)" }} />
              <div className="stat-item">
                <Gauge size={14} color="var(--accent-green)" />
                <span>Limit:</span>
                <span className="stat-value">Unlimited</span>
              </div>
            </div>

            <button className="btn-add-download" onClick={handleAddDownload}>
              <Plus size={16} strokeWidth={3} />
              <span>Add Download</span>
            </button>
          </div>
        </header>

        {/* Content Area */}
        <main className="content-area">
          {activeCategory === "settings" ? (
            <div className="welcome-box">
              <h2 className="welcome-title">Settings Panel</h2>
              <p className="welcome-desc">Configuration details will be set up in Phase 5.</p>
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
                    
                    const progressPercent = d.total_size > 0 
                      ? Math.min(100, Math.floor((d.downloaded / d.total_size) * 100))
                      : 0;

                    return (
                      <div className="download-card" key={d.id.toString()}>
                        {/* Header */}
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

                        {/* Progress */}
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

                        {/* Actions */}
                        <div className="card-actions">
                          {isDownloading && (
                            <button className="action-btn" onClick={() => handlePause(d.id)}>
                              <Pause size={14} />
                              <span>Pause</span>
                            </button>
                          )}
                          {isPaused && (
                            <button className="action-btn" onClick={() => handleResume(d.id)}>
                              <Play size={14} />
                              <span>Resume</span>
                            </button>
                          )}
                          <button className="action-btn action-btn-danger" onClick={() => handleCancel(d.id)}>
                            <X size={14} />
                            <span>Remove</span>
                          </button>
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
    </div>
  );
}

export default App;
