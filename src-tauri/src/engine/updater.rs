//! 在线更新器、SHA-256 校验网关与静默覆盖安装引擎 (Updater & Checksum Gate)。
//!
//! 提供 GitHub Releases 资产匹配、SHA-256 完整性清单解析、本地文件验签网关与静默安装拉起。
//!
//! @author Ateng
//! @since 2026-10-08

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use thiserror::Error;

/// 更新器统一业务错误
#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("文件 IO 读写异常: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP 网络请求异常: {0}")]
    Network(String),
    #[error("SHA-256 完整性校验失败: 期望 {expected}, 实际 {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("未找到对应平台的安装包发布资产: {0}")]
    AssetNotFound(String),
    #[error("安装包子进程拉起异常: {0}")]
    LaunchFailed(String),
}

/// 更新下载进度事件载荷 (IPC 事件通道: update-download-progress)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateProgressPayload {
    /// 已接收字节数
    pub received_bytes: u64,
    /// 文件总字节数 (未知时为 0)
    pub total_bytes: u64,
    /// 下载百分比 (0.0 ~ 100.0)
    pub percentage: f64,
    /// 当前阶段状态 ("downloading" | "verifying" | "ready" | "error")
    pub status: String,
}

/// 解析 GitHub Releases 发布的 `checksums.txt` 清单
///
/// 契约支持格式：
/// `<sha256_hex>  <filename>` 或 `<sha256_hex> <filename>`
///
/// @param manifest_str 清单原始文本
/// @return 文件名到 SHA-256 小写哈希字符串的映射集合
pub fn parse_checksums(manifest_str: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in manifest_str.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 {
            let hash = parts[0].to_ascii_lowercase();
            let filename = parts[1].trim().to_string();
            map.insert(filename, hash);
        }
    }
    map
}

/// 校验指定本地文件的 SHA-256 哈希值与预期值是否相符 (Checksum Gate)
///
/// @param file_path 待校验的本地文件路径
/// @param expected_hex 预期的 64 位十六进制 SHA-256 字符串 (大小写不敏感)
/// @return 若匹配返回 Ok(true)，若不匹配返回 Err(UpdaterError::ChecksumMismatch)
pub fn verify_file_sha256(file_path: &Path, expected_hex: &str) -> Result<bool, UpdaterError> {
    let mut file = File::open(file_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let actual_hash = crate::engine::hash::sha256_hex(&buffer);
    let expected_lower = expected_hex.trim().to_ascii_lowercase();

    if actual_hash == expected_lower {
        Ok(true)
    } else {
        Err(UpdaterError::ChecksumMismatch {
            expected: expected_lower,
            actual: actual_hash,
        })
    }
}

/// 根据操作系统与架构在资产名称列表中匹配目标安装包
///
/// 规则：
/// - Windows x86_64: 优先匹配以 `-Windows-x64-Setup.exe` 或 `-Setup.exe` 结尾且包含 Windows 的项
///
/// @param os 目标操作系统标识 (如 "windows", "macos", "linux")
/// @param arch 目标硬件架构 (如 "x86_64", "aarch64")
/// @param assets 候选资产文件名列表
/// @return 命中的目标资产文件名
pub fn match_target_asset(os: &str, arch: &str, assets: &[String]) -> Option<String> {
    let os_lower = os.to_ascii_lowercase();
    let arch_lower = arch.to_ascii_lowercase();

    if os_lower.contains("windows") || os_lower.contains("win") {
        if arch_lower.contains("64") {
            // 优先精确匹配 Setup.exe
            if let Some(a) = assets.iter().find(|s| s.contains("Windows") && s.contains("x64") && s.ends_with(".exe")) {
                return Some(a.clone());
            }
            if let Some(a) = assets.iter().find(|s| s.ends_with("-Setup.exe") || s.ends_with(".exe")) {
                return Some(a.clone());
            }
        }
    }
    None
}

/// 启动 Windows 静默覆盖安装器 (In-place Upgrade)
///
/// 传递 `/S` 静默参数，触发 `hooks.nsh` 的 `PREINSTALL` 优雅/强制终止旧进程并原地写入新版本。
///
/// @param installer_path 安装包本地路径
pub fn launch_silent_installer(installer_path: &Path) -> Result<(), UpdaterError> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW = 0x08000000
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let mut cmd = std::process::Command::new(installer_path);
        cmd.arg("/S");
        cmd.creation_flags(CREATE_NO_WINDOW);

        cmd.spawn()
            .map_err(|e| UpdaterError::LaunchFailed(format!("拉起静默安装器失败: {e}")))?;
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = installer_path;
        Err(UpdaterError::LaunchFailed("非 Windows 平台不支持 NSIS 静默升级".into()))
    }
}

/// 原生多通道流式容灾下载引擎 (Native Download Engine)
///
/// 依次尝试候选下载端点，首选官方直链（可自动穿透系统代理），连接受阻时自动无缝降级至备用国内加速镜像通道。
///
/// @param endpoints 候选下载端点列表 (通道名称, 下载URL)
/// @param target_path 本地写入临时文件路径
/// @param on_progress 进度回调闭包 (已接收字节数, 总字节数, 当前活动通道名称)
/// @return 成功命中的下载通道名称
pub async fn download_with_fallback<F>(
    endpoints: &[(String, String)],
    target_path: &Path,
    mut on_progress: F,
) -> Result<String, UpdaterError>
where
    F: FnMut(u64, u64, &str),
{
    use futures_util::StreamExt;
    use std::io::Write;
    use std::time::Duration;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(6))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| UpdaterError::Network(format!("创建 HTTP 客户端失败: {e}")))?;

    let mut last_err = String::from("无可用候选节点");

    for (name, url) in endpoints {
        on_progress(0, 0, name);

        let response = match client.get(url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    resp
                } else {
                    last_err = format!("通道 {name} 返回 HTTP 状态码: {}", resp.status());
                    continue;
                }
            }
            Err(e) => {
                last_err = format!("通道 {name} 连接异常: {e}");
                continue;
            }
        };

        let total_bytes = response.content_length().unwrap_or(0);
        let mut file = match File::create(target_path) {
            Ok(f) => f,
            Err(e) => return Err(UpdaterError::Io(e)),
        };

        let mut stream = response.bytes_stream();
        let mut received: u64 = 0;
        let mut stream_failed = false;

        while let Some(item) = stream.next().await {
            match item {
                Ok(chunk) => {
                    if let Err(e) = file.write_all(&chunk) {
                        let _ = std::fs::remove_file(target_path);
                        return Err(UpdaterError::Io(e));
                    }
                    received += chunk.len() as u64;
                    on_progress(received, total_bytes, name);
                }
                Err(e) => {
                    last_err = format!("通道 {name} 流式读取中断: {e}");
                    stream_failed = true;
                    break;
                }
            }
        }

        if stream_failed {
            let _ = std::fs::remove_file(target_path);
            continue;
        }

        let _ = file.flush();

        if total_bytes > 0 && received < total_bytes {
            last_err = format!("通道 {name} 数据接收不完整: {received}/{total_bytes}");
            let _ = std::fs::remove_file(target_path);
            continue;
        }

        return Ok(name.clone());
    }

    Err(UpdaterError::Network(format!("所有加速与直连通道连接均受限: {last_err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_checksums_manifest() {
        let sample = r#"
# 官方 SHA-256 校验清单
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  Clip-1.2.3-Windows-x64-Setup.exe
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad Clip-1.2.3-macOS-arm64.dmg
        "#;

        let map = parse_checksums(sample);
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("Clip-1.2.3-Windows-x64-Setup.exe").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            map.get("Clip-1.2.3-macOS-arm64.dmg").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_verify_file_sha256_match_and_tampered() {
        let temp_dir = std::env::temp_dir().join("clip_updater_test");
        let _ = std::fs::create_dir_all(&temp_dir);
        let test_file = temp_dir.join("test_installer.bin");

        // 写入测试内容 "hello clip updater"
        let mut f = File::create(&test_file).unwrap();
        f.write_all(b"hello clip updater").unwrap();
        drop(f);

        // 计算预期 SHA-256: 7ec3...
        let expected_hash = crate::engine::hash::sha256_hex(b"hello clip updater");

        // 1. 正确哈希校验放行
        let ok = verify_file_sha256(&test_file, &expected_hash).unwrap();
        assert!(ok, "合法文件必须通过 Checksum Gate");

        // 2. 伪造或损坏哈希被阻断
        let err = verify_file_sha256(&test_file, "0000000000000000000000000000000000000000000000000000000000000000");
        assert!(err.is_err(), "篡改哈希必须被 Checksum Gate 拦截");
        match err.unwrap_err() {
            UpdaterError::ChecksumMismatch { expected, actual } => {
                assert_eq!(expected, "0000000000000000000000000000000000000000000000000000000000000000");
                assert_eq!(actual, expected_hash);
            }
            other => panic!("意外的错误类型: {:?}", other),
        }

        let _ = std::fs::remove_file(&test_file);
    }

    #[test]
    fn test_match_target_asset() {
        let assets = vec![
            "checksums.txt".to_string(),
            "Clip-1.2.3-macOS-arm64.dmg".to_string(),
            "Clip-1.2.3-Linux-amd64.deb".to_string(),
            "Clip-1.2.3-Windows-x64-Setup.exe".to_string(),
        ];

        let matched = match_target_asset("windows", "x86_64", &assets);
        assert_eq!(
            matched,
            Some("Clip-1.2.3-Windows-x64-Setup.exe".to_string())
        );

        let none = match_target_asset("linux", "x86_64", &assets);
        assert_eq!(none, None);
    }
}
