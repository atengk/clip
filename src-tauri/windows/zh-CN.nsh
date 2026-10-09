; Clip NSIS 简体中文本地化资源 (Tauri 官方全量运行时消息与现代浅色定制向导)
; @author Ateng
; @since 2026-10-07

; 1. 运行中程序检测与关闭提示 (彻底解决卸载空白弹窗 Bug)
LangString appRunning ${LANG_SIMPCHINESE} "{{product_name}} 正在运行！请关闭后再试。"
LangString appRunningOkKill ${LANG_SIMPCHINESE} "{{product_name}} 正在后台运行！$\r$\n点击“确定”将自动关闭进程并继续卸载，点击“取消”中止卸载。"
LangString failedToKillApp ${LANG_SIMPCHINESE} "无法关闭 {{product_name}} 进程。请手动退出托盘图标后再试。"

; 2. 安装与维护向导文本
LangString addOrReinstall ${LANG_SIMPCHINESE} "添加/重新安装组件"
LangString alreadyInstalled ${LANG_SIMPCHINESE} "已安装"
LangString alreadyInstalledLong ${LANG_SIMPCHINESE} "${PRODUCTNAME} ${VERSION} 已经安装。请选择您要执行的操作后点击“下一步”继续。"
LangString chooseMaintenanceOption ${LANG_SIMPCHINESE} "选择要执行的维护操作。"
LangString choowHowToInstall ${LANG_SIMPCHINESE} "选择您要安装 ${PRODUCTNAME} 的方式。"
LangString createDesktop ${LANG_SIMPCHINESE} "创建桌面快捷方式"
LangString dontUninstall ${LANG_SIMPCHINESE} "请勿卸载"
LangString dontUninstallDowngrade ${LANG_SIMPCHINESE} "请勿卸载（此安装程序禁止未卸载就进行版本降级操作）"
LangString newerVersionInstalled ${LANG_SIMPCHINESE} "系统中已安装更新版本的 ${PRODUCTNAME}！不建议安装旧版本。如需安装，请先卸载当前版本。"
LangString older ${LANG_SIMPCHINESE} "旧版本"
LangString olderOrUnknownVersionInstalled ${LANG_SIMPCHINESE} "系统中已存在版本为 $R4 的 ${PRODUCTNAME}。建议先卸载当前版本后再安装。"
LangString silentDowngrades ${LANG_SIMPCHINESE} "降级操作已禁用，无法静默安装，请使用图形向导界面。$\r$\n"
LangString unableToUninstall ${LANG_SIMPCHINESE} "无法卸载！"
LangString uninstallApp ${LANG_SIMPCHINESE} "卸载 ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_SIMPCHINESE} "安装前卸载"
LangString unknown ${LANG_SIMPCHINESE} "未知"

; 3. WebView2 运行时支持
LangString installingWebview2 ${LANG_SIMPCHINESE} "正在安装 WebView2 运行时..."
LangString webview2AbortError ${LANG_SIMPCHINESE} "无法安装 WebView2！没有它应用程序无法运行。请尝试重新运行安装程序。"
LangString webview2DownloadError ${LANG_SIMPCHINESE} "错误：无法下载 WebView2 - $0"
LangString webview2DownloadSuccess ${LANG_SIMPCHINESE} "WebView2 引导程序下载成功"
LangString webview2Downloading ${LANG_SIMPCHINESE} "正在下载 WebView2 引导程序..."
LangString webview2InstallError ${LANG_SIMPCHINESE} "错误：安装 WebView2 失败，错误代码：$1"
LangString webview2InstallSuccess ${LANG_SIMPCHINESE} "成功安装 WebView2"

; 4. Clip 定制浅色向导页面文案
LangString launchClipNow ${LANG_SIMPCHINESE} "立即运行 Clip (推荐)"
LangString finishPageDescription ${LANG_SIMPCHINESE} "${PRODUCTNAME} 已成功安装到您的计算机！$\r$\n$\r$\n• 唤出面板：随时按下 Alt + V 快速调出或隐藏$\r$\n• 后台守护：关闭后常驻系统托盘，静默零打扰$\r$\n• 极速粘贴：支持 1~9 数字键直接回填"
LangString dirPageTopText ${LANG_SIMPCHINESE} "安装程序将把 ${PRODUCTNAME} 安装至当前用户免提权安全目录。"
LangString dirPageDestText ${LANG_SIMPCHINESE} "目标文件夹 (当前用户免管理员权限)"
LangString uninstPageTopText ${LANG_SIMPCHINESE} "安装向导即将从计算机中卸载 ${PRODUCTNAME}。"
LangString uninstPageLocationText ${LANG_SIMPCHINESE} "卸载目录："
LangString deleteAppData ${LANG_SIMPCHINESE} "同时清除本地剪贴板历史与自定义短语 (默认保留)"
