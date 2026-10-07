; Clip NSIS 安装生命周期钩子与界面宏配置

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

; 4. 安装生命周期宏
!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
