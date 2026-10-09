; Clip NSIS English Localization Resources (Tauri Full Runtime Messages & Modern Light Setup)
; @author Ateng
; @since 2026-10-07

; 1. Running Application Detection & Close Prompt
LangString appRunning ${LANG_ENGLISH} "{{product_name}} is currently running! Please close it and try again."
LangString appRunningOkKill ${LANG_ENGLISH} "{{product_name}} is currently running in background!$\r$\nClick 'OK' to terminate the process and continue, or 'Cancel' to abort."
LangString failedToKillApp ${LANG_ENGLISH} "Unable to terminate {{product_name}}. Please exit from the tray icon manually and try again."

; 2. Installation & Maintenance Wizard Messages
LangString addOrReinstall ${LANG_ENGLISH} "Add/Reinstall Components"
LangString alreadyInstalled ${LANG_ENGLISH} "Already Installed"
LangString alreadyInstalledLong ${LANG_ENGLISH} "${PRODUCTNAME} ${VERSION} is already installed. Select the operation you wish to perform and click Next to continue."
LangString chooseMaintenanceOption ${LANG_ENGLISH} "Select maintenance options to perform."
LangString choowHowToInstall ${LANG_ENGLISH} "Choose how you want to install ${PRODUCTNAME}."
LangString createDesktop ${LANG_ENGLISH} "Create Desktop Shortcut"
LangString dontUninstall ${LANG_ENGLISH} "Do Not Uninstall"
LangString dontUninstallDowngrade ${LANG_ENGLISH} "Do Not Uninstall (Downgrading without uninstallation is prohibited)"
LangString newerVersionInstalled ${LANG_ENGLISH} "A newer version of ${PRODUCTNAME} is already installed! It is not recommended to install an older version. Please uninstall the current version first."
LangString older ${LANG_ENGLISH} "older"
LangString olderOrUnknownVersionInstalled ${LANG_ENGLISH} "A version ($R4) of ${PRODUCTNAME} already exists. It is recommended to uninstall it first. Select your action and click Next."
LangString silentDowngrades ${LANG_ENGLISH} "Downgrades are disabled for silent installation. Please run the interactive graphical installer.$\r$\n"
LangString unableToUninstall ${LANG_ENGLISH} "Unable to Uninstall!"
LangString uninstallApp ${LANG_ENGLISH} "Uninstall ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_ENGLISH} "Uninstall Before Installing"
LangString unknown ${LANG_ENGLISH} "unknown"

; 3. WebView2 Runtime Support
LangString installingWebview2 ${LANG_ENGLISH} "Installing WebView2 Runtime..."
LangString webview2AbortError ${LANG_ENGLISH} "Failed to install WebView2! The application cannot run without it. Please restart setup."
LangString webview2DownloadError ${LANG_ENGLISH} "Error: Unable to download WebView2 - $0"
LangString webview2DownloadSuccess ${LANG_ENGLISH} "WebView2 bootstrapper downloaded successfully"
LangString webview2Downloading ${LANG_ENGLISH} "Downloading WebView2 bootstrapper..."
LangString webview2InstallError ${LANG_ENGLISH} "Error: Failed to install WebView2, exit code: $1"
LangString webview2InstallSuccess ${LANG_ENGLISH} "Successfully installed WebView2"

; 4. Clip Custom Light Wizard Page Text
LangString launchClipNow ${LANG_ENGLISH} "Launch Clip now (Recommended)"
LangString finishPageDescription ${LANG_ENGLISH} "${PRODUCTNAME} has been successfully installed on your computer!$\r$\n$\r$\n• Toggle Panel: Press Alt + V anytime to show or hide$\r$\n• Tray Daemon: Silently guards in system tray when closed$\r$\n• Instant Paste: Use 1~9 keys for instant pasting"
LangString dirPageTopText ${LANG_ENGLISH} "Setup will install ${PRODUCTNAME} into the current user directory without requiring elevation."
LangString dirPageDestText ${LANG_ENGLISH} "Destination Folder (Current User - No Elevation Required)"
LangString uninstPageTopText ${LANG_ENGLISH} "Setup will uninstall ${PRODUCTNAME} from your computer."
LangString uninstPageLocationText ${LANG_ENGLISH} "Uninstalling from:"
LangString deleteAppData ${LANG_ENGLISH} "Also remove clipboard history and custom snippets (Preserved by default)"
