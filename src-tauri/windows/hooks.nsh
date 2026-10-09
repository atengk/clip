; Clip NSIS 安装生命周期钩子与界面宏配置
; @author Ateng
; @since 2026-10-08

; 1. 目标目录选择页免管理员权限安全提示
!define MUI_DIRECTORYPAGE_TEXT_TOP "$(dirPageTopText)"
!define MUI_DIRECTORYPAGE_TEXT_DESTINATION "$(dirPageDestText)"

; 2. 完成页 Alt+V 快捷指引卡片与默认勾选自启动 (精简行高，彻底杜绝控件重叠)
!define MUI_FINISHPAGE_RUN_TEXT "$(launchClipNow)"
!define MUI_FINISHPAGE_RUN_CHECKED
!define MUI_FINISHPAGE_TEXT "$(finishPageDescription)"

; 3. 卸载确认页顶部说明与目录标签
!define MUI_UNCONFIRMPAGE_TEXT_TOP "$(uninstPageTopText)"
!define MUI_UNCONFIRMPAGE_TEXT_LOCATION "$(uninstPageLocationText)"

; 4. 安装生命周期宏 (Process Self-Healing & In-place Upgrade)
!macro NSIS_HOOK_PREINSTALL
  DetailPrint "正在检测并退出正在运行的 Clip 进程以解除文件锁定..."
  ; 1. 进程自愈：终止运行中的 Clip.exe 避免 Windows 独占写入锁定
  nsExec::Exec 'taskkill /F /IM Clip.exe'
  ; 2. 安全缓冲等待 500ms 确保文件句柄彻底释放
  Sleep 500

  ; 3. 纯覆盖就地升级模式 (Pure In-Place Overwrite Mode):
  ; 临时移除旧版本的 UninstallString 键值，彻底屏蔽安装器对旧版卸载向导 (Uninstall.exe) 的链式调用
  ; 实现极速就地二进制直接覆盖，绝不走任何卸载流程，100% 保障用户数据与极速无感体验
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Clip"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\com.clip.app"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; 安装完成自动拉起新版本 Clip.exe (无论静默安装 /S 还是交互向导)
  IfSilent 0 +3
    ExecShell "" "$INSTDIR\Clip.exe"
    Goto +2
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; 卸载前同样确保关闭 Clip.exe 进程
  nsExec::Exec 'taskkill /F /IM Clip.exe'
  Sleep 500
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; 用户数据安全契约: 默认保留 %APPDATA%\com.clip.app，防止误删剪贴板历史
  ; 路径定义: $APPDATA\com.clip.app
!macroend
