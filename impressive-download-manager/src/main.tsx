import React, { Component, ErrorInfo, ReactNode } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
}

class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null,
    errorInfo: null,
  };

  public static getDerivedStateFromError(error: Error): Partial<State> {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("Uncaught React Error:", error, errorInfo);
    this.setState({ errorInfo });
  }

  public render() {
    if (this.state.hasError) {
      return (
        <div style={{
          padding: "24px",
          backgroundColor: "#0d1117",
          color: "#f8fafc",
          fontFamily: "monospace",
          height: "100vh",
          overflow: "auto",
          boxSizing: "border-box"
        }}>
          <h2 style={{ color: "#ef4444", marginTop: 0 }}>⚠️ Runtime Execution Error</h2>
          <div style={{
            background: "rgba(239, 68, 68, 0.1)",
            border: "1px solid #ef4444",
            borderRadius: "8px",
            padding: "16px",
            marginBottom: "16px"
          }}>
            <strong>Error:</strong> {this.state.error?.toString()}
          </div>
          <h3>Component Stack:</h3>
          <pre style={{
            background: "#1e293b",
            padding: "12px",
            borderRadius: "6px",
            fontSize: "0.85rem",
            whiteSpace: "pre-wrap"
          }}>
            {this.state.errorInfo?.componentStack || "No component stack available"}
          </pre>
          <button
            onClick={() => window.location.reload()}
            style={{
              marginTop: "16px",
              padding: "10px 20px",
              background: "#3b82f6",
              color: "#fff",
              border: "none",
              borderRadius: "6px",
              cursor: "pointer",
              fontWeight: 600
            }}
          >
            Reload Window
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}

// Global unhandled promise rejection listener
window.addEventListener("unhandledrejection", (event) => {
  console.error("Unhandled Promise Rejection:", event.reason);
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
