@echo off
setlocal enabledelayedexpansion

:: Set console output code page to UTF-8 for clean box-drawing & colors
chcp 65001 >nul 2>&1

title Impressive Download Manager - Setup ^& Dependencies Installer

:: ==============================================================================
:: Color Scheme ^& UI Formatting Constants (ANSI escape codes)
:: ==============================================================================
set "ESC="
set "C_RESET=[0m"
set "C_BOLD=[1m"
set "C_CYAN=[38;2;6;182;212m"
set "C_BLUE=[38;2;59;130;246m"
set "C_GREEN=[38;2;16;185;129m"
set "C_YELLOW=[38;2;245;158;11m"
set "C_RED=[38;2;239;68;68m"
set "C_DIM=[38;2;148;163;184m"
set "C_WHITE=[38;2;248;250;252m"

cls
echo %C_CYAN%
echo ╔══════════════════════════════════════════════════════════════════════════════════════╗
echo ║                                                                                      ║
echo ║   %C_BOLD%%C_WHITE%  IMPRESSIVE DOWNLOAD MANAGER %C_RESET%%C_CYAN%                                            ║
echo ║   %C_DIM%  Automated Development Environment ^& Dependency Setup                       %C_CYAN%║
echo ║                                                                                      ║
echo ╚══════════════════════════════════════════════════════════════════════════════════════╝
echo %C_RESET%

:: ------------------------------------------------------------------------------
:: Step 1: Privilege Verification
:: ------------------------------------------------------------------------------
echo %C_BOLD%%C_BLUE%[STEP 1/6]%C_RESET% %C_WHITE%Checking Administrator Privileges...%C_RESET%
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo.
    echo %C_RED%[✘] ERROR: Administrator privileges required!%C_RESET%
    echo %C_DIM%Please right-click %C_WHITE%install_dependencies.bat%C_DIM% and select %C_BOLD%"Run as Administrator"%C_RESET%.
    echo.
    pause
    exit /b 1
)
echo %C_GREEN%  [✔] Running with Administrator privileges.%C_RESET%
echo.

:: ------------------------------------------------------------------------------
:: Step 2: Package Manager Selection ^& Installation
:: ------------------------------------------------------------------------------
echo %C_BOLD%%C_BLUE%[STEP 2/6]%C_RESET% %C_WHITE%Package Manager Configuration%C_RESET%
echo %C_DIM%  Checking available package managers (winget, chocolatey)...%C_RESET%

set "PKG_MANAGER=none"

where winget >nul 2>&1
if %errorLevel% equ 0 (
    set "HAS_WINGET=1"
    echo %C_GREEN%  [✔] Windows Package Manager (winget) detected.%C_RESET%
) else (
    set "HAS_WINGET=0"
)

where choco >nul 2>&1
if %errorLevel% equ 0 (
    set "HAS_CHOCO=1"
    echo %C_GREEN%  [✔] Chocolatey Package Manager (choco) detected.%C_RESET%
) else (
    set "HAS_CHOCO=0"
)

echo.
echo %C_BOLD%  Select preferred package manager for setup:%C_RESET%
echo   %C_CYAN%[1]%C_RESET% Windows Package Manager (winget) %C_DIM%- Recommended for modern Windows%C_RESET%
echo   %C_CYAN%[2]%C_RESET% Chocolatey (choco) %C_DIM%- Will auto-install Chocolatey if missing%C_RESET%
echo.

set /p "PKG_CHOICE=  Enter selection [1-2] (Default: 1): "
if "%PKG_CHOICE%"=="2" (
    set "PKG_MANAGER=choco"
) else (
    if "!HAS_WINGET!"=="1" (
        set "PKG_MANAGER=winget"
    ) else (
        echo %C_YELLOW%  [!] winget not found. Switching to Chocolatey...%C_RESET%
        set "PKG_MANAGER=choco"
    )
)

if "!PKG_MANAGER!"=="choco" (
    if "!HAS_CHOCO!"=="0" (
        echo %C_YELLOW%  [➜] Installing Chocolatey Package Manager...%C_RESET%
        powershell -NoProfile -ExecutionPolicy Bypass -Command "[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072; iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))"
        if exist "%ALLUSERSPROFILE%\chocolatey\bin\RefreshEnv.cmd" (
            call "%ALLUSERSPROFILE%\chocolatey\bin\RefreshEnv.cmd"
        )
    )
)

echo %C_GREEN%  [✔] Selected Package Manager: %C_BOLD%!PKG_MANAGER!%C_RESET%
echo.

:: ------------------------------------------------------------------------------
:: Step 3: Visual Studio C++ Build Tools Verification
:: ------------------------------------------------------------------------------
echo %C_BOLD%%C_BLUE%[STEP 3/6]%C_RESET% %C_WHITE%Visual Studio C++ Build Tools Check%C_RESET%
set "HAS_MSVC=0"
set "VS_WHERE_PATH=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"

if exist "!VS_WHERE_PATH!" (
    for /f "usebackq tokens=*" %%i in (`"!VS_WHERE_PATH!" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
        if not "%%i"=="" set "HAS_MSVC=1"
    )
)

if "!HAS_MSVC!"=="1" (
    echo %C_GREEN%  [✔] C++ Build Tools / MSVC environment detected.%C_RESET%
) else (
    echo %C_YELLOW%  [!] C++ Build Tools missing (Required for Rust ^& Tauri binary compilation).%C_RESET%
    echo %C_CYAN%  [➜] Installing Visual Studio 2022 Build Tools (C++ Workload)...%C_RESET%
    if "!PKG_MANAGER!"=="winget" (
        winget install --id Microsoft.VisualStudio.2022.BuildTools --silent --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"
    ) else (
        choco install visualstudio2022buildtools visualstudio2022-workload-vctools -y
    )
    echo %C_GREEN%  [✔] Visual Studio C++ Build Tools setup completed.%C_RESET%
)
echo.

:: ------------------------------------------------------------------------------
:: Step 4: Core Runtime ^& Toolchain Verification (Node.js LTS, Rust, NSIS, WiX)
:: ------------------------------------------------------------------------------
echo %C_BOLD%%C_BLUE%[STEP 4/6]%C_RESET% %C_WHITE%Core Runtimes ^& Build Toolchains Setup%C_RESET%

:: 1. Node.js LTS
echo %C_DIM%  ▸ Checking Node.js (LTS)...%C_RESET%
where node >nul 2>&1
if %errorLevel% equ 0 (
    for /f "tokens=*" %%v in ('node -v') do set "NODE_VER=%%v"
    echo %C_GREEN%    [✔] Node.js is already installed (!NODE_VER!).%C_RESET%
) else (
    echo %C_CYAN%    [➜] Installing Node.js LTS...%C_RESET%
    if "!PKG_MANAGER!"=="winget" (
        winget install --id OpenJS.NodeJS.LTS -e --accept-source-agreements --accept-package-agreements
    ) else (
        choco install nodejs-lts -y
    )
)

:: 2. Rustup ^& Rust Toolchain
echo %C_DIM%  ▸ Checking Rust toolchain (rustup / rustc)...%C_RESET%
where rustc >nul 2>&1
if %errorLevel% equ 0 (
    for /f "tokens=*" %%v in ('rustc --version') do set "RUST_VER=%%v"
    echo %C_GREEN%    [✔] Rust is already installed (!RUST_VER!).%C_RESET%
) else (
    echo %C_CYAN%    [➜] Installing Rustup toolchain...%C_RESET%
    curl -sSf -o "%TEMP%\rustup-init.exe" https://win.rustup.rs/
    if exist "%TEMP%\rustup-init.exe" (
        "%TEMP%\rustup-init.exe" -y --default-toolchain stable-x86_64-pc-windows-msvc
        del "%TEMP%\rustup-init.exe" >nul 2>&1
    ) else (
        if "!PKG_MANAGER!"=="winget" (
            winget install --id Rustlang.Rustup -e --accept-source-agreements --accept-package-agreements
        ) else (
            choco install rustup -y
        )
    )
    echo %C_GREEN%    [✔] Rustup toolchain installed.%C_RESET%
)

:: 3. NSIS (Windows Installer Bundler)
echo %C_DIM%  ▸ Checking NSIS (Nullsoft Scriptable Install System)...%C_RESET%
where makensis >nul 2>&1
if %errorLevel% equ 0 (
    echo %C_GREEN%    [✔] NSIS is already installed.%C_RESET%
) else (
    echo %C_CYAN%    [➜] Installing NSIS...%C_RESET%
    if "!PKG_MANAGER!"=="winget" (
        winget install --id NSIS.NSIS -e --accept-source-agreements --accept-package-agreements
    ) else (
        choco install nsis -y
    )
)

:: 4. WiX Toolset (.msi Bundler)
echo %C_DIM%  ▸ Checking WiX Toolset...%C_RESET%
where candle >nul 2>&1
if %errorLevel% equ 0 (
    echo %C_GREEN%    [✔] WiX Toolset is already installed.%C_RESET%
) else (
    echo %C_CYAN%    [➜] Installing WiX Toolset...%C_RESET%
    if "!PKG_MANAGER!"=="winget" (
        winget install --id WiXToolset.WiXToolset -e --accept-source-agreements --accept-package-agreements
    ) else (
        choco install wix -y
    )
)
echo.

:: ------------------------------------------------------------------------------
:: Step 5: Optional Containerization (Docker Desktop)
:: ------------------------------------------------------------------------------
echo %C_BOLD%%C_BLUE%[STEP 5/6]%C_RESET% %C_WHITE%Optional Tooling: Docker Desktop%C_RESET%
where docker >nul 2>&1
if %errorLevel% equ 0 (
    echo %C_GREEN%  [✔] Docker Desktop is already installed.%C_RESET%
) else (
    echo %C_DIM%  Docker Desktop allows containerized builds ^& testing.%C_RESET%
    set /p "DOCKER_CHOICE=  Do you want to install Docker Desktop? [y/N]: "
    if /i "!DOCKER_CHOICE!"=="y" (
        echo %C_CYAN%  [➜] Installing Docker Desktop...%C_RESET%
        if "!PKG_MANAGER!"=="winget" (
            winget install --id Docker.DockerDesktop -e --accept-source-agreements --accept-package-agreements
        ) else (
            choco install docker-desktop -y
        )
        echo %C_GREEN%  [✔] Docker Desktop installed successfully.%C_RESET%
    ) else (
        echo %C_DIM%  [-] Skipped Docker Desktop installation.%C_RESET%
    )
)
echo.

:: ------------------------------------------------------------------------------
:: Step 6: Summary ^& Completion Checklist
:: ------------------------------------------------------------------------------
echo %C_BOLD%%C_BLUE%[STEP 6/6]%C_RESET% %C_WHITE%Installation Summary ^& Environment Checklist%C_RESET%
echo %C_CYAN%┌──────────────────────────────────────────────────────────────────────────────┐%C_RESET%
echo %C_CYAN%│ %C_BOLD%%C_WHITE%STATUS   COMPONENT                             DETECTION STATUS           %C_CYAN%│%C_RESET%
echo %C_CYAN%├──────────────────────────────────────────────────────────────────────────────┤%C_RESET%

where node >nul 2>&1
if %errorLevel% equ 0 (
    echo %C_CYAN%│ %C_GREEN% [✔]    %C_WHITE%Node.js (LTS Runtime)                  %C_GREEN%Installed                  %C_CYAN%│%C_RESET%
) else (
    echo %C_CYAN%│ %C_RED% [✘]    %C_WHITE%Node.js (LTS Runtime)                  %C_RED%Not Found                  %C_CYAN%│%C_RESET%
)

where rustc >nul 2>&1
if %errorLevel% equ 0 (
    echo %C_CYAN%│ %C_GREEN% [✔]    %C_WHITE%Rust Toolchain (rustc / cargo)         %C_GREEN%Installed                  %C_CYAN%│%C_RESET%
) else (
    echo %C_CYAN%│ %C_RED% [✘]    %C_WHITE%Rust Toolchain (rustc / cargo)         %C_RED%Not Found                  %C_CYAN%│%C_RESET%
)

if "!HAS_MSVC!"=="1" (
    echo %C_CYAN%│ %C_GREEN% [✔]    %C_WHITE%VS C++ Build Tools (MSVC)               %C_GREEN%Installed                  %C_CYAN%│%C_RESET%
) else (
    echo %C_CYAN%│ %C_YELLOW% [!]    %C_WHITE%VS C++ Build Tools (MSVC)               %C_YELLOW%Action Required            %C_CYAN%│%C_RESET%
)

where makensis >nul 2>&1
if %errorLevel% equ 0 (
    echo %C_CYAN%│ %C_GREEN% [✔]    %C_WHITE%NSIS (Setup Installer Creator)        %C_GREEN%Installed                  %C_CYAN%│%C_RESET%
) else (
    echo %C_CYAN%│ %C_DIM% [-]    %C_WHITE%NSIS (Setup Installer Creator)        %C_DIM%Optional                   %C_CYAN%│%C_RESET%
)

where docker >nul 2>&1
if %errorLevel% equ 0 (
    echo %C_CYAN%│ %C_GREEN% [✔]    %C_WHITE%Docker Desktop                        %C_GREEN%Installed                  %C_CYAN%│%C_RESET%
) else (
    echo %C_CYAN%│ %C_DIM% [-]    %C_WHITE%Docker Desktop                        %C_DIM%Skipped/Optional           %C_CYAN%│%C_RESET%
)

echo %C_CYAN%└──────────────────────────────────────────────────────────────────────────────┘%C_RESET%

echo.
echo %C_BOLD%%C_GREEN%✔ SETUP COMPLETED!%C_RESET%
echo %C_DIM%==============================================================================%C_RESET%
echo %C_YELLOW%IMPORTANT:%C_RESET% %C_WHITE%Please restart your terminal/command prompt or PC so%C_RESET%
echo %C_WHITE%your system's PATH variables refresh before running %C_CYAN%npm run dev%C_WHITE% or %C_CYAN%npm run build%C_WHITE%.%C_RESET%
echo %C_DIM%==============================================================================%C_RESET%
echo.
pause

