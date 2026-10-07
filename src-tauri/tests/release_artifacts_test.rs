//! CI 全平台发布产物规范化命名与 SHA-256 清单集成验收测试
//!
//! @author Ateng
//! @since 2026-10-07

use std::collections::HashMap;
use clip_lib::engine::hash::sha256_hex;

/// 产物规范化映射器
fn normalize_artifact_name(file: &str, version: &str) -> Option<String> {
    if file.ends_with("setup.exe") || file.ends_with("Setup.exe") || file.ends_with(".exe") {
        Some(format!("Clip-{version}-Windows-x64-Setup.exe"))
    } else if file.ends_with(".msi") {
        Some(format!("Clip-{version}-Windows-x64.msi"))
    } else if file.ends_with(".dmg") {
        Some(format!("Clip-{version}-macOS-arm64.dmg"))
    } else if file.ends_with(".app.tar.gz") || (file.contains("darwin") && file.ends_with(".tar.gz")) {
        Some(format!("Clip-{version}-macOS-arm64.app.tar.gz"))
    } else if file.ends_with(".AppImage") {
        Some(format!("Clip-{version}-Linux-x86_64.AppImage"))
    } else if file.ends_with(".deb") {
        Some(format!("Clip-{version}-Linux-amd64.deb"))
    } else if file.ends_with(".rpm") {
        Some(format!("Clip-{version}-Linux-x86_64.rpm"))
    } else {
        None
    }
}

#[test]
fn test_all_7_platform_artifacts_normalization() {
    let version = "1.0.0";

    // 1. 模拟 Tauri 各平台构建输出的原始非标/默认文件名
    let raw_files = vec![
        "clip_1.0.0_x64-setup.exe",
        "clip_1.0.0_x64_en-US.msi",
        "clip_1.0.0_aarch64.dmg",
        "clip_1.0.0_aarch64.app.tar.gz",
        "clip_1.0.0_amd64.AppImage",
        "clip_1.0.0_amd64.deb",
        "clip-1.0.0-1.x86_64.rpm",
        // 干扰项 (应被过滤清理)
        "clip.sig",
        "build.log",
    ];

    let mut normalized_map = HashMap::new();
    for raw in &raw_files {
        if let Some(norm) = normalize_artifact_name(raw, version) {
            normalized_map.insert(*raw, norm);
        }
    }

    // 2. 验证正好映射为 7 个标准产物
    assert_eq!(normalized_map.len(), 7, "必须且仅能匹配 7 个标准分发包");

    let expected_names = [
        "Clip-1.0.0-Windows-x64-Setup.exe",
        "Clip-1.0.0-Windows-x64.msi",
        "Clip-1.0.0-macOS-arm64.dmg",
        "Clip-1.0.0-macOS-arm64.app.tar.gz",
        "Clip-1.0.0-Linux-x86_64.AppImage",
        "Clip-1.0.0-Linux-amd64.deb",
        "Clip-1.0.0-Linux-x86_64.rpm",
    ];

    for expected in &expected_names {
        assert!(
            normalized_map.values().any(|v| v == expected),
            "缺少目标产物: {}",
            expected
        );
    }

    // 3. 验证无遗留小写开头产物
    for norm in normalized_map.values() {
        assert!(
            norm.starts_with("Clip-"),
            "规范名称必须以 'Clip-' 语义前缀开头: {}",
            norm
        );
    }

    // 4. 模拟 SHA-256 清单编译与自检校验
    let mut checksum_lines = Vec::new();
    let mut file_payloads = HashMap::new();

    for name in &expected_names {
        let fake_content = format!("Mock binary payload for {}", name);
        let hash_hex = sha256_hex(fake_content.as_bytes());

        checksum_lines.push(format!("{}  {}", hash_hex, name));
        file_payloads.insert(*name, fake_content);
    }

    let checksums_txt = checksum_lines.join("\n");

    // 校验 checksums.txt 每行格式并验证哈希匹配 (模拟 sha256sum -c checksums.txt)
    for line in checksums_txt.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts.len(), 2, "每行必须包含哈希与文件名");
        let hash = parts[0];
        let filename = parts[1];

        assert_eq!(hash.len(), 64, "SHA-256 哈希值必须为 64 位十六进制字符");
        let content = file_payloads.get(filename).expect("目标文件存在");
        let calc_hash = sha256_hex(content.as_bytes());
        assert_eq!(hash, calc_hash, "文件哈希校验必须匹配: {}", filename);
    }
}
