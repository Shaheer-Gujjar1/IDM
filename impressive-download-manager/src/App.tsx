import { useState } from "react";
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
  Shield,
  Zap,
  Globe
} from "lucide-react";
import "./App.css";

interface Category {
  id: string;
  name: string;
  icon: React.ReactNode;
  count: number;
}

function App() {
  const [activeCategory, setActiveCategory] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");

  const mainCategories: Category[] = [
    { id: "all", name: "All Downloads", icon: <Layers size={18} />, count: 0 },
    { id: "downloading", name: "Downloading", icon: <Activity size={18} />, count: 0 },
    { id: "completed", name: "Completed", icon: <CheckCircle2 size={18} />, count: 0 },
    { id: "paused", name: "Paused", icon: <Clock size={18} />, count: 0 },
    { id: "trash", name: "Trash", icon: <Trash2 size={18} />, count: 0 },
  ];

  const fileCategories: Category[] = [
    { id: "videos", name: "Videos", icon: <Film size={16} />, count: 0 },
    { id: "audio", name: "Audio", icon: <Music size={16} />, count: 0 },
    { id: "documents", name: "Documents", icon: <FileText size={16} />, count: 0 },
    { id: "archives", name: "Archives", icon: <Archive size={16} />, count: 0 },
  ];

  const handleAddDownload = () => {
    // Will be fully wired in Phase 4
    console.log("Add Download clicked");
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
              <span className="menu-item-count">{cat.count}</span>
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
              <span className="menu-item-count">{cat.count}</span>
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
                <span className="stat-value">0 KB/s</span>
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
            <div className="welcome-box">
              <h2 className="welcome-title">Impressive Download Manager</h2>
              <p className="welcome-desc">
                Welcome to the next-generation, Rust-powered download engine. Zero bloat, maximum performance, and stunning design.
              </p>

              <div className="welcome-features">
                <div className="feature-card">
                  <div className="feature-icon-wrapper">
                    <Zap size={20} />
                  </div>
                  <div className="feature-details">
                    <h4>Multi-threaded Engine</h4>
                    <p>Rust-driven concurrency divides downloads into parallel chunks for maximum speed.</p>
                  </div>
                </div>

                <div className="feature-card">
                  <div className="feature-icon-wrapper">
                    <Shield size={20} />
                  </div>
                  <div className="feature-details">
                    <h4>Safe & Resource Friendly</h4>
                    <p>Unlike resource-heavy legacy engines, Impressive DM operates with near-zero memory overhead.</p>
                  </div>
                </div>

                <div className="feature-card">
                  <div className="feature-icon-wrapper">
                    <Globe size={20} />
                  </div>
                  <div className="feature-details">
                    <h4>Browser Capture</h4>
                    <p>Integrates directly with browsers to capture high-definition streams and direct links seamlessly.</p>
                  </div>
                </div>

                <div className="feature-card">
                  <div className="feature-icon-wrapper">
                    <Layers size={20} />
                  </div>
                  <div className="feature-details">
                    <h4>Bento Dashboard</h4>
                    <p>A minimalist interface focusing entirely on the information you need, when you need it.</p>
                  </div>
                </div>
              </div>
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

export default App;
