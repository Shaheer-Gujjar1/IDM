@echo off
:: ==============================================================================
:: Impressive Download Manager - Windows Dependencies Automated Installer
:: ==============================================================================

echo [IDM Installer] Checking for Administrator privileges...
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo [ERROR] This installer requires administrative rights.
    echo Please right-click this .bat file and select "Run as Administrator".
    pause
    exit /b 1
)

echo [IDM Installer] Administrator privileges verified.
echo [IDM Installer] Checking for Chocolatey package manager...

where choco >nul 2>&1
if %errorLevel% neq 0 (
    echo [IDM Installer] Chocolatey not found. Installing Chocolatey...
    powershell -NoProfile -InputFormat None -ExecutionPolicy Bypass -Command "[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072; iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))"
    if %errorLevel% neq 0 (
        echo [ERROR] Failed to install Chocolatey. Please install it manually from https://chocolatey.org/
        pause
        exit /b 1
      )
    :: Refresh environment variables for the current cmd session
    call "%ALLUSERSPROFILE%\chocolatey\bin\RefreshEnv.cmd"
) else (
    echo [IDM Installer] Chocolatey is already installed.
)

echo [IDM Installer] Installing compilation & packaging dependencies...
echo [IDM Installer] This might take several minutes depending on network speed.

:: Install Node.js LTS, Rust, VS2022 Build Tools (with C++ workload), NSIS, and WiX Toolset
choco install nodejs-lts rustup visualstudio2022buildtools visualstudio2022-workload-vctools nsis wix -y

if %errorLevel% neq 0 (
    echo [WARNING] One or more installations encountered errors. Please check the logs above.
) else (
    echo [IDM Installer] All dependencies installed successfully!
)

echo.
echo ==============================================================================
echo IMPORTANT: Please restart your terminal/IDE or restart your PC to refresh
echo your system's PATH variables before compiling the application.
echo ==============================================================================
pause
