//! 灾备归档与数据导入导出引擎 (Backup Archive)。
//!
//! 实现剪贴板 SQLite 结构化数据与 Blob Store 图片目录的自包含 `.clipbak` (ZIP) 完整打包与无损解包还原。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::storage::{ClipboardEntry, Snippet, Storage, StorageError};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// 灾备归档包元数据清单
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    /// 备份包格式规范版本号 (默认 "1.0.0")
    pub version: String,
    /// 导出生成的毫秒时间戳
    pub created_at: i64,
    /// 包含的剪贴板历史条目总数
    #[serde(alias = "total_entries")]
    pub entry_count: usize,
    /// 包含的常用短语模板总数
    #[serde(alias = "total_snippets")]
    pub snippet_count: usize,
    /// 包含的图片 Blob 文件总数
    #[serde(alias = "total_blobs")]
    pub blob_count: usize,
}

/// 灾备归档统一错误枚举
#[derive(Debug, Error)]
pub enum BackupError {
    #[error("文件读写 IO 异常: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP 归档解压缩异常: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("JSON 序列化解析异常: {0}")]
    Json(#[from] serde_json::Error),
    #[error("持久化存储异常: {0}")]
    Storage(#[from] StorageError),
    #[error("归档包格式无效或已损坏: {0}")]
    InvalidArchive(String),
}

/// 灾备归档与数据恢复引擎
pub struct BackupArchive;

impl BackupArchive {
    /// 一键导出数据至自包含 .clipbak 压缩包 (AC-3)
    ///
    /// 打包 SQLite 历史记录、常用短语模板及关联图片 Blob 文件。
    ///
    /// @param storage 数据存储层契约引用
    /// @param blob_dir 本地图片 Blob 存储目录
    /// @param target_path 目标导出 .clipbak 文件路径
    /// @return 导出的备份元数据清单
    pub fn export_archive(
        storage: &dyn Storage,
        blob_dir: &Path,
        target_path: &Path,
    ) -> Result<BackupManifest, BackupError> {
        // 1. 抓取待备份的全量数据实体
        let entries = storage.get_recent_entries(usize::MAX)?;
        let snippets = storage.get_all_snippets()?;

        let mut blob_files: Vec<(String, Vec<u8>)> = Vec::new();
        if blob_dir.exists() {
            if let Ok(dir_entries) = std::fs::read_dir(blob_dir) {
                for item in dir_entries.flatten() {
                    let path = item.path();
                    if path.is_file() {
                        if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                            if let Ok(bytes) = std::fs::read(&path) {
                                blob_files.push((file_name.to_string(), bytes));
                            }
                        }
                    }
                }
            }
        }

        let manifest = BackupManifest {
            version: "1.0.0".into(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            entry_count: entries.len(),
            snippet_count: snippets.len(),
            blob_count: blob_files.len(),
        };

        // 2. 构造 ZIP 归档包
        let file = File::create(target_path)?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // 写入 manifest.json
        zip.start_file("manifest.json", options)?;
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
        zip.write_all(&manifest_bytes)?;

        // 写入 entries.json
        zip.start_file("entries.json", options)?;
        let entries_bytes = serde_json::to_vec_pretty(&entries)?;
        zip.write_all(&entries_bytes)?;

        // 写入 snippets.json
        zip.start_file("snippets.json", options)?;
        let snippets_bytes = serde_json::to_vec_pretty(&snippets)?;
        zip.write_all(&snippets_bytes)?;

        // 写入关联图片 Blobs
        for (file_name, data) in blob_files {
            let zip_path = format!("blobs/{file_name}");
            zip.start_file(zip_path, options)?;
            zip.write_all(&data)?;
        }

        zip.finish()?;
        Ok(manifest)
    }

    /// 从 .clipbak 压缩包导入并完整还原图文历史与常用短语 (AC-4)
    ///
    /// @param storage 目标数据存储层契约引用
    /// @param blob_dir 目标本地图片 Blob 存储目录
    /// @param source_path 待导入的 .clipbak 文件路径
    /// @return 还原的备份元数据清单
    pub fn import_archive(
        storage: &dyn Storage,
        blob_dir: &Path,
        source_path: &Path,
    ) -> Result<BackupManifest, BackupError> {
        let file = File::open(source_path)?;
        let mut zip = ZipArchive::new(file)?;

        // 1. 读取并校验 manifest.json
        let manifest: BackupManifest = {
            let mut manifest_file = zip
                .by_name("manifest.json")
                .map_err(|_| BackupError::InvalidArchive("缺失 manifest.json 清单文件".into()))?;
            let mut content = String::new();
            manifest_file.read_to_string(&mut content)?;
            serde_json::from_str(&content)?
        };

        // 2. 解压并恢复图片 Blobs，防范 Zip Slip 路径穿透风险
        let _ = std::fs::create_dir_all(blob_dir);
        for i in 0..zip.len() {
            let mut entry_file = zip.by_index(i)?;
            if let Some(enclosed_path) = entry_file.enclosed_name() {
                if enclosed_path.starts_with("blobs") && !entry_file.is_dir() {
                    if let Ok(relative_sub) = enclosed_path.strip_prefix("blobs") {
                        let target_path = blob_dir.join(relative_sub);
                        let mut buffer = Vec::new();
                        entry_file.read_to_end(&mut buffer)?;
                        let _ = std::fs::write(target_path, buffer);
                    }
                }
            }
        }

        // 3. 读取并恢复常用短语 snippets.json
        if let Ok(mut snippets_file) = zip.by_name("snippets.json") {
            let mut content = String::new();
            snippets_file.read_to_string(&mut content)?;
            if let Ok(snippets) = serde_json::from_str::<Vec<Snippet>>(&content) {
                for sn in snippets {
                    // 若已存在相同快捷指令或内容的短语则保留更新
                    let _ = storage.save_snippet(None, &sn.title, &sn.content, &sn.shortcut);
                }
            }
        }

        // 4. 读取并恢复历史条目 entries.json
        if let Ok(mut entries_file) = zip.by_name("entries.json") {
            let mut content = String::new();
            entries_file.read_to_string(&mut content)?;
            if let Ok(entries) = serde_json::from_str::<Vec<ClipboardEntry>>(&content) {
                // 按原有时间正序恢复插入，以保持最新条目在前
                let mut sorted_entries = entries;
                sorted_entries.sort_by_key(|e| e.created_at);

                for item in sorted_entries {
                    if item.entry_type == "image" {
                        if let Ok(inserted) = storage.insert_image(&item.content, "", "", "") {
                            if item.is_pinned {
                                let _ = storage.toggle_pin(inserted.id);
                            }
                        }
                    } else {
                        // 还原纯文本时动态生成拼音简拼与全拼索引 (AC-4 保持检索完备)
                        let pinyin_first =
                            crate::engine::pinyin::PinyinMatcher::to_first_letters_index(&item.content);
                        let pinyin_full =
                            crate::engine::pinyin::PinyinMatcher::to_full_pinyin_index(&item.content);
                        if let Ok(inserted) = storage.insert_text(
                            &item.content,
                            &item.content,
                            &pinyin_first,
                            &pinyin_full,
                        ) {
                            if item.is_pinned {
                                let _ = storage.toggle_pin(inserted.id);
                            }
                        }
                    }
                }
            }
        }

        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::SqliteStorage;
    use std::sync::Arc;

    #[test]
    fn test_backup_archive_export_and_import() {
        let temp_dir = std::env::temp_dir().join(format!(
            "clip_backup_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&temp_dir);

        let src_blob_dir = temp_dir.join("src_blobs");
        let _ = std::fs::create_dir_all(&src_blob_dir);
        let src_storage = Arc::new(SqliteStorage::new_in_memory().unwrap());

        // 1. 插入文本历史与置顶条目
        let t1 = src_storage
            .insert_text("常驻守护与灾备归档", "常驻守护与灾备归档", "czsh", "changzhu")
            .unwrap();
        src_storage.toggle_pin(t1.id).unwrap();

        let _ = src_storage
            .insert_text("第二条普通文本", "第二条普通文本", "de", "dier")
            .unwrap();

        // 2. 插入图片历史与图片 Blob 文件
        let dummy_bmp = b"BMdummy_bitmap_data_bytes_123456";
        let img_hash = "mock_hash_image_001";
        std::fs::write(src_blob_dir.join(format!("{img_hash}.bmp")), dummy_bmp).unwrap();
        let _ = src_storage.insert_image(img_hash, "", "", "").unwrap();

        // 3. 插入常用短语模板
        let _ = src_storage
            .save_snippet(None, "每日总结", "今日完成：{current_date}", "daily")
            .unwrap();

        // 4. 导出为 .clipbak 压缩包
        let archive_path = temp_dir.join("test_backup.clipbak");
        let manifest =
            BackupArchive::export_archive(&*src_storage, &src_blob_dir, &archive_path).unwrap();

        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.entry_count, 3);
        assert_eq!(manifest.snippet_count, 4); // 3条系统预置短语 + 1条测试新增短语
        assert_eq!(manifest.blob_count, 1);
        assert!(archive_path.exists());

        // 5. 在全新的隔离环境进行解包还原
        let dst_blob_dir = temp_dir.join("dst_blobs");
        let dst_storage = Arc::new(SqliteStorage::new_in_memory().unwrap());

        let imported_manifest =
            BackupArchive::import_archive(&*dst_storage, &dst_blob_dir, &archive_path).unwrap();
        assert_eq!(imported_manifest.entry_count, 3);

        // 6. 验证还原后的数据完整性
        let restored_entries = dst_storage.get_recent_entries(10).unwrap();
        assert_eq!(restored_entries.len(), 3);

        // 验证置顶条目保留了置顶状态
        let pinned_item = restored_entries.iter().find(|e| e.content == "常驻守护与灾备归档").unwrap();
        assert!(pinned_item.is_pinned);

        // 验证短语模板完整还原
        let restored_snippets = dst_storage.get_all_snippets().unwrap();
        let daily_snippet = restored_snippets
            .iter()
            .find(|s| s.title == "每日总结")
            .expect("未能找到还原的'每日总结'短语模板");
        assert_eq!(daily_snippet.shortcut, "daily");
        assert_eq!(daily_snippet.content, "今日完成：{current_date}");

        // 验证图片 Blob 二进制文件原样恢复
        let restored_blob_path = dst_blob_dir.join(format!("{img_hash}.bmp"));
        assert!(restored_blob_path.exists());
        let restored_blob_data = std::fs::read(restored_blob_path).unwrap();
        assert_eq!(restored_blob_data, dummy_bmp);

        // 清理测试目录
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
