//! 在线覆盖升级与安装包调度 IPC 命令层。
//!
//! 负责接收前端下载的发布构件二进制数据、调用 Checksum Gate 强校验、写入安全临时文件并在退出前触发静默覆盖安装。
//!
//! @author Ateng
//! @since 2026-10-08

use crate::engine::updater::{
    download_with_fallback, launch_silent_installer, verify_file_sha256, UpdateProgressPayload,
    UpdaterError,
};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::Emitter;

/// 准备并校验临时更新安装包（纯函数接缝，供单元测试与命令层复用）
///
/// 1. 净化文件名，防御路径穿越；
/// 2. 写入指定的临时目录；
/// 3. 若提供预期哈希值，执行 Checksum Gate 校验，失败则自动删除临时文件；
/// 4. 返回就绪的临时安装文件路径。
///
/// @param temp_dir 目标临时目录
/// @param data 安装包二进制字节流
/// @param file_name 目标文件名
/// @param expected_sha256 可选的预期 SHA-256 哈希值
/// @return 校验通过的临时文件路径
pub fn prepare_and_verify_update_file(
    temp_dir: &Path,
    data: &[u8],
    file_name: &str,
    expected_sha256: Option<&str>,
) -> Result<PathBuf, UpdaterError> {
    if data.is_empty() {
        return Err(UpdaterError::AssetNotFound("安装包数据为空".into()));
    }

    // 1. 过滤并净化文件名，防御目录穿越
    let safe_name = Path::new(file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Clip-Update-Setup.exe");

    let _ = std::fs::create_dir_all(temp_dir);
    let target_path = temp_dir.join(safe_name);

    // 2. 写入临时文件
    {
        let mut file = File::create(&target_path)?;
        file.write_all(data)?;
        file.flush()?;
    }

    // 3. 执行 Checksum Gate 完整性强校验
    if let Some(expected) = expected_sha256 {
        if let Err(err) = verify_file_sha256(&target_path, expected) {
            // 校验失败立即自洁临时文件
            let _ = std::fs::remove_file(&target_path);
            return Err(err);
        }
    }

    Ok(target_path)
}



/// 执行原地覆盖升级命令 (In-place Upgrade)
///
/// @param app_handle Tauri 应用句柄
/// @param data 前端下载完成的安装包字节数组
/// @param file_name 安装包名称 (如 Clip-1.2.3-Windows-x64-Setup.exe)
/// @param expected_sha256 可选的官方 SHA-256 校验值
#[tauri::command]
pub async fn execute_in_place_update(
    app_handle: tauri::AppHandle,
    data: Vec<u8>,
    file_name: String,
    expected_sha256: Option<String>,
) -> Result<(), String> {
    let temp_dir = std::env::temp_dir().join("clip_updates");

    // 1. 准备并验证临时文件
    let target_path = prepare_and_verify_update_file(
        &temp_dir,
        &data,
        &file_name,
        expected_sha256.as_deref(),
    )
    .map_err(|e| format!("更新包准备或校验失败: {e}"))?;

    // 2. 触发静默安装器 (Setup.exe /S)
    launch_silent_installer(&target_path)
        .map_err(|e| format!("启动静默升级安装器失败: {e}"))?;

    // 3. 平滑退出当前应用，交由安装器完成原地覆写与自动拉起
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        app_handle.exit(0);
    });

    Ok(())
}

/// 原生下载并执行原地升级命令 (Native Download & In-place Upgrade)
///
/// 1. 净化文件名并创建临时升级目录；
/// 2. 构建下载候选端点（包含官方直链与多组国内备用镜像）；
/// 3. 使用原生 reqwest 流式异步下载，实时发射 `update-download-progress` 事件给前端；
/// 4. 下载完毕后自动执行 SHA-256 完整性强校验 (Checksum Gate)；
/// 5. 校验通过拉起静默安装器并退出当前进程完成无感覆盖升级。
#[tauri::command]
pub async fn download_and_install_update(
    app_handle: tauri::AppHandle,
    url: String,
    file_name: String,
    expected_sha256: Option<String>,
) -> Result<(), String> {
    let temp_dir = std::env::temp_dir().join("clip_updates");
    let _ = std::fs::create_dir_all(&temp_dir);

    let safe_name = Path::new(&file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Clip-Update-Setup.exe");
    let target_path = temp_dir.join(safe_name);

    // 构建候选端点：官方直链优先（自动走系统代理），备用国内加速节点
    let mut endpoints: Vec<(String, String)> = Vec::new();
    endpoints.push(("GitHub 节点 (代理优先)".into(), url.clone()));
    endpoints.push(("加速镜像通道 A (ghproxy.cn)".into(), format!("https://ghproxy.cn/{}", url)));
    endpoints.push(("加速镜像通道 B (mirror.ghproxy.com)".into(), format!("https://mirror.ghproxy.com/{}", url)));
    endpoints.push(("加速镜像通道 C (ghproxy.net)".into(), format!("https://ghproxy.net/{}", url)));

    let app_handle_clone = app_handle.clone();
    let hit_endpoint = download_with_fallback(&endpoints, &target_path, move |received, total, endpoint_name| {
        let pct = if total > 0 {
            ((received as f64 / total as f64) * 100.0).min(100.0)
        } else {
            0.0
        };
        let _ = app_handle_clone.emit(
            "update-download-progress",
            UpdateProgressPayload {
                received_bytes: received,
                total_bytes: total,
                percentage: pct,
                status: format!("正在下载 ({:.0}%) [{}]", pct, endpoint_name),
            },
        );
    })
    .await
    .map_err(|e| format!("下载更新包失败: {e}"))?;

    // 校验 SHA-256 (Checksum Gate)
    if let Some(ref expected) = expected_sha256 {
        let _ = app_handle.emit(
            "update-download-progress",
            UpdateProgressPayload {
                received_bytes: 0,
                total_bytes: 0,
                percentage: 100.0,
                status: "正在校验安装包完整性...".into(),
            },
        );
        if let Err(err) = verify_file_sha256(&target_path, expected) {
            let _ = std::fs::remove_file(&target_path);
            return Err(format!("安装包 SHA-256 校验未通过: {err}"));
        }
    }

    let _ = app_handle.emit(
        "update-download-progress",
        UpdateProgressPayload {
            received_bytes: 0,
            total_bytes: 0,
            percentage: 100.0,
            status: format!("下载与校验完成 (节点: {})，正在启动覆盖安装...", hit_endpoint),
        },
    );

    // 触发静默安装器
    launch_silent_installer(&target_path).map_err(|e| format!("启动静默升级安装器失败: {e}"))?;

    // 平滑退出当前应用，交由安装器完成原地覆写与自动拉起
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        app_handle.exit(0);
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_and_verify_update_file_success_and_tampered() {
        let temp_dir = std::env::temp_dir().join("clip_cmd_updater_test");
        let dummy_data = b"clip fake installer payload 2026";
        let expected_hash = crate::engine::hash::sha256_hex(dummy_data);

        // 1. 正常校验通过并落盘
        let path = prepare_and_verify_update_file(
            &temp_dir,
            dummy_data,
            "../../Clip-Test-Setup.exe", // 携带路径穿越字符测试安全净化
            Some(&expected_hash),
        )
        .expect("合法数据必须校验通过");

        assert_eq!(path.file_name().unwrap(), "Clip-Test-Setup.exe");
        assert!(path.exists(), "临时安装包文件必须存在");

        // 2. 篡改校验失败并自动删除临时文件
        let tampered_result = prepare_and_verify_update_file(
            &temp_dir,
            dummy_data,
            "Clip-Tampered-Setup.exe",
            Some("1111111111111111111111111111111111111111111111111111111111111111"),
        );

        assert!(tampered_result.is_err(), "篡改哈希必须拦截");
        let tampered_path = temp_dir.join("Clip-Tampered-Setup.exe");
        assert!(!tampered_path.exists(), "校验失败后临时文件必须被自洁清理");

        // 清理测试目录
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
