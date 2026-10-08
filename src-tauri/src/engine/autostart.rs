//! 开机静默后台自启动管理器 (Autostart Manager)。
//!
//! 在 Windows 平台通过当前用户注册表 (Run 键) 管理随系统静默自启状态，在非 Windows 平台提供兼容桩。
//!
//! @author Ateng
//! @since 2026-10-06


/// 自启动配置与管理器
pub struct AutostartManager;

impl AutostartManager {
    /// 检查当前应用程序是否已启用开机自启动
    pub fn is_enabled() -> bool {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            let output = std::process::Command::new("reg")
                .args([
                    "query",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "Clip",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output();

            match output {
                Ok(out) => out.status.success(),
                Err(_) => false,
            }
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// 设置开机静默后台自启动状态 (AC-2)
    ///
    /// @param enable true 为开启，false 为禁用
    /// @return 是否操作成功
    pub fn set_enabled(enable: bool) -> Result<(), String> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            if enable {
                let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
                let exe_str = current_exe.to_string_lossy().to_string();
                let cmd_val = format!("\"{exe_str}\" --silent");

                let status = std::process::Command::new("reg")
                    .args([
                        "add",
                        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                        "/v",
                        "Clip",
                        "/t",
                        "REG_SZ",
                        "/d",
                        &cmd_val,
                        "/f",
                    ])
                    .creation_flags(CREATE_NO_WINDOW)
                    .status()
                    .map_err(|e| format!("执行注册表写入失败: {e}"))?;

                if status.success() {
                    Ok(())
                } else {
                    Err("注册表写入命令返回非零错误".into())
                }
            } else {
                let _ = std::process::Command::new("reg")
                    .args([
                        "delete",
                        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                        "/v",
                        "Clip",
                        "/f",
                    ])
                    .creation_flags(CREATE_NO_WINDOW)
                    .status();
                Ok(())
            }
        }
        #[cfg(not(windows))]
        {
            let _ = enable;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_query_does_not_panic() {
        // 保证查询状态在任意环境均不 panic 且安全返回布尔值
        let enabled = AutostartManager::is_enabled();
        println!("Autostart enabled status: {enabled}");
    }
}
