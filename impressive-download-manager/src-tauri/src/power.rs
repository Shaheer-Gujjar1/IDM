use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AutoShutdownConfig {
    pub action: String, // "none", "shutdown", "sleep", "hibernate", "quit"
    pub enabled: bool,
    pub countdown_seconds: u32,
}

impl Default for AutoShutdownConfig {
    fn default() -> Self {
        Self {
            action: "none".to_string(),
            enabled: false,
            countdown_seconds: 30,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShutdownCountdownPayload {
    pub action: String,
    pub seconds_left: u32,
    pub is_cancelled: bool,
}

pub struct PowerManager {
    pub config: tokio::sync::RwLock<AutoShutdownConfig>,
    pub is_countdown_active: Arc<AtomicBool>,
    pub cancel_flag: Arc<AtomicBool>,
}

impl PowerManager {
    pub fn new() -> Self {
        Self {
            config: tokio::sync::RwLock::new(AutoShutdownConfig::default()),
            is_countdown_active: Arc::new(AtomicBool::new(false)),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn get_config(&self) -> AutoShutdownConfig {
        self.config.read().await.clone()
    }

    pub async fn set_config(&self, config: AutoShutdownConfig) {
        *self.config.write().await = config;
    }

    pub fn cancel_countdown(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
        self.is_countdown_active.store(false, Ordering::Relaxed);
        let _ = cancel_system_shutdown();
    }

    pub async fn on_tasks_evaluated(
        &self,
        has_active_tasks: bool,
        has_completed_tasks: bool,
        app_handle: Option<&tauri::AppHandle>,
    ) {
        // Only trigger if no downloads are currently active, at least one completed, and countdown not already running
        if has_active_tasks || !has_completed_tasks {
            return;
        }

        let cfg = self.config.read().await.clone();
        if !cfg.enabled || cfg.action == "none" {
            return;
        }

        if self.is_countdown_active.load(Ordering::Relaxed) {
            return;
        }

        self.is_countdown_active.store(true, Ordering::Relaxed);
        self.cancel_flag.store(false, Ordering::Relaxed);

        let countdown_total = if cfg.countdown_seconds > 0 { cfg.countdown_seconds } else { 30 };
        let action = cfg.action.clone();
        let cancel_flag = self.cancel_flag.clone();
        let is_active = self.is_countdown_active.clone();
        let app_handle_opt = app_handle.cloned();

        println!("[PowerManager] All downloads completed. Initiating '{}' countdown ({}s)...", action, countdown_total);

        tokio::spawn(async move {
            for sec in (1..=countdown_total).rev() {
                if cancel_flag.load(Ordering::Relaxed) {
                    println!("[PowerManager] Shutdown action '{}' was cancelled by user.", action);
                    if let Some(ref handle) = app_handle_opt {
                        let _ = handle.emit("auto-shutdown-countdown", ShutdownCountdownPayload {
                            action: action.clone(),
                            seconds_left: 0,
                            is_cancelled: true,
                        });
                    }
                    is_active.store(false, Ordering::Relaxed);
                    return;
                }

                if let Some(ref handle) = app_handle_opt {
                    let _ = handle.emit("auto-shutdown-countdown", ShutdownCountdownPayload {
                        action: action.clone(),
                        seconds_left: sec,
                        is_cancelled: false,
                    });
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            if !cancel_flag.load(Ordering::Relaxed) {
                println!("[PowerManager] Countdown finished! Executing power action: {}", action);
                let _ = execute_power_action(&action);
            }

            is_active.store(false, Ordering::Relaxed);
        });
    }
}

/// Executes the designated system power action across Linux, Windows, and macOS
pub fn execute_power_action(action: &str) -> Result<(), String> {
    match action.to_lowercase().as_str() {
        "shutdown" => {
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("shutdown")
                    .args(["/s", "/t", "0", "/c", "All downloads completed in IDM."])
                    .spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("osascript")
                    .args(["-e", "tell app \"System Events\" to shut down"])
                    .spawn();
            }
            #[cfg(target_os = "linux")]
            {
                if std::process::Command::new("systemctl").arg("poweroff").spawn().is_err() {
                    let _ = std::process::Command::new("shutdown").args(["-h", "now"]).spawn();
                }
            }
            Ok(())
        }
        "sleep" => {
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("rundll32.exe")
                    .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                    .spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("pmset")
                    .arg("sleepnow")
                    .spawn();
            }
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("systemctl")
                    .arg("suspend")
                    .spawn();
            }
            Ok(())
        }
        "hibernate" => {
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("shutdown")
                    .args(["/h"])
                    .spawn();
            }
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("systemctl")
                    .arg("hibernate")
                    .spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("pmset")
                    .arg("sleepnow")
                    .spawn();
            }
            Ok(())
        }
        "quit" => {
            std::process::exit(0);
        }
        _ => Ok(()),
    }
}

/// Cancels scheduled OS shutdown (e.g. `shutdown /a` on Windows or `shutdown -c` on Linux)
pub fn cancel_system_shutdown() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("shutdown").arg("/a").spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("shutdown").arg("-c").spawn();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_shutdown_config_defaults() {
        let default_cfg = AutoShutdownConfig::default();
        assert_eq!(default_cfg.action, "none");
        assert!(!default_cfg.enabled);
        assert_eq!(default_cfg.countdown_seconds, 30);
    }

    #[tokio::test]
    async fn test_power_manager_cancellation_flow() {
        let pm = PowerManager::new();
        pm.set_config(AutoShutdownConfig {
            action: "sleep".to_string(),
            enabled: true,
            countdown_seconds: 5,
        }).await;

        let cfg = pm.get_config().await;
        assert_eq!(cfg.action, "sleep");
        assert!(cfg.enabled);

        // Test cancel
        pm.cancel_countdown();
        assert!(pm.cancel_flag.load(Ordering::Relaxed));
        assert!(!pm.is_countdown_active.load(Ordering::Relaxed));
    }
}
