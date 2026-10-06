//! 剪贴板核心业务状态机引擎 (ClipboardEngine)。
//!
//! 负责剪贴板内容捕获、时间窗口回填防环、Bump-to-Top 策略、置顶管理与超大文本 Payload Guard 熔断保护。
//!
//! @author Ateng
//! @since 2026-10-06

pub mod hash;
pub mod pinyin;
pub mod privacy;
pub mod queue;
pub mod snippet;
pub mod transform;

use crate::engine::pinyin::PinyinMatcher;
use crate::engine::privacy::PrivacyFilter;
use crate::engine::queue::{PasteQueueManager, QueueItem, QueueStatus};
use crate::engine::snippet::{SnippetContext, SnippetEngine};
use crate::engine::transform::{TextTransformer, TransformAction};
use crate::pal::{PalError, PlatformDriver};
use crate::storage::{ClipboardEntry, Snippet, Storage, StorageError};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

/// 触发超大文本熔断保护的阈值 (2MB = 2,097,152 字节)
pub const LARGE_PAYLOAD_THRESHOLD_BYTES: usize = 2 * 1024 * 1024;

/// 超大文本索引截断上限 (200KB = 204,800 字节)
pub const MAX_INDEX_PAYLOAD_BYTES: usize = 200 * 1024;

/// 拼音索引提取安全截断字符数，防止超大文本引起多音字组合计算卡顿
pub const MAX_PINYIN_SOURCE_CHARS: usize = 2000;

/// 默认非置顶条目 LRU 容量上限
pub const DEFAULT_MAX_CAPACITY: usize = 1000;

/// 核心引擎统一错误枚举
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("平台驱动异常: {0}")]
    Platform(#[from] PalError),
    #[error("存储引擎异常: {0}")]
    Storage(#[from] StorageError),
    #[error("文件读写 IO 异常: {0}")]
    Io(#[from] std::io::Error),
    #[error("目标条目未找到: {0}")]
    EntryNotFound(i64),
    #[error("格式转换失败: {0}")]
    Transform(String),
}

/// 图片元数据与 Base64 视图模型
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImageDetail {
    pub data_url: String,
    pub width: i32,
    pub height: i32,
    pub file_size: u64,
}

/// 剪贴板状态机核心引擎
pub struct ClipboardEngine {
    driver: Arc<dyn PlatformDriver>,
    storage: Arc<dyn Storage>,
    last_captured_text: Mutex<Option<String>>,
    paste_suppression: Mutex<Option<(String, Instant)>>,
    paste_suppression_image: Mutex<Option<(String, Instant)>>,
    max_capacity: usize,
    privacy_filter: PrivacyFilter,
    blob_dir: PathBuf,
    queue_manager: Arc<PasteQueueManager>,
}

impl ClipboardEngine {
    /// 构造全新的核心业务引擎
    ///
    /// @param driver 平台抽象驱动实例
    /// @param storage 存储层实例
    pub fn new(driver: Arc<dyn PlatformDriver>, storage: Arc<dyn Storage>) -> Self {
        let default_blob_dir = std::env::temp_dir().join("clip_blobs");
        let _ = std::fs::create_dir_all(&default_blob_dir);
        Self {
            driver,
            storage,
            last_captured_text: Mutex::new(None),
            paste_suppression: Mutex::new(None),
            paste_suppression_image: Mutex::new(None),
            max_capacity: DEFAULT_MAX_CAPACITY,
            privacy_filter: PrivacyFilter::new(),
            blob_dir: default_blob_dir,
            queue_manager: Arc::new(PasteQueueManager::new()),
        }
    }

    /// 设置自定义图片 Blob 存储目录（用于测试隔离或自定义数据目录）
    ///
    /// @param blob_dir 图片文件存放根目录
    /// @return 链式返回 Self
    pub fn with_blob_dir(mut self, blob_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&blob_dir);
        self.blob_dir = blob_dir;
        self
    }

    /// 设置自定义 LRU 容量上限（用于测试和调优）
    ///
    /// @param capacity 容量上限值
    /// @return 链式返回 Self
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.max_capacity = capacity;
        self
    }

    /// 设置自定义隐私安全过滤器（用于测试或黑名单配置）
    ///
    /// @param filter 隐私安全过滤器
    /// @return 链式返回 Self
    pub fn with_privacy_filter(mut self, filter: PrivacyFilter) -> Self {
        self.privacy_filter = filter;
        self
    }

    /// 清理无引用孤立图片文件 (Blob GC)
    ///
    /// @return 成功清理的孤立图片文件数量
    pub fn prune_orphan_blobs(&self) -> Result<usize, EngineError> {
        let active_blobs: std::collections::HashSet<String> = self
            .storage
            .get_all_image_contents()?
            .into_iter()
            .collect();

        let mut removed_count = 0;
        if let Ok(entries) = std::fs::read_dir(&self.blob_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                        let hash = file_name.strip_suffix(".bmp").unwrap_or(file_name);
                        if !active_blobs.contains(hash) && std::fs::remove_file(&path).is_ok() {
                            removed_count += 1;
                        }
                    }
                }
            }
        }
        Ok(removed_count)
    }

    /// 处理剪贴板变更事件
    ///
    /// 包含隐私过滤拦截、防环校验、已有历史 Bump-to-Top、超大文本 Payload Guard 索引熔断及 LRU 淘汰。
    ///
    /// @return 捕获或跃升的剪贴板实体；若被拦截或抑制则返回 Ok(None)
    pub fn handle_clipboard_change(&self) -> Result<Option<ClipboardEntry>, EngineError> {
        // 1. 协议级标记与进程黑名单隐私拦截 (Privacy Filter)
        let is_ignored = self.driver.is_clipboard_ignored()?;
        let source_process = self.driver.get_clipboard_source_process()?;
        if self.privacy_filter.should_ignore(is_ignored, source_process.as_deref()) {
            return Ok(None);
        }

        // 2. 优先检查并读取剪贴板图片二进制数据
        if let Some(image_data) = self.driver.read_image()? {
            if !image_data.is_empty() {
                let hash = hash::sha256_hex(&image_data);

                // 图片回填防环检验 (800ms)
                {
                    let mut supp_guard = self.paste_suppression_image.lock().unwrap();
                    if let Some((ref suppressed_hash, timestamp)) = *supp_guard {
                        if suppressed_hash == &hash && timestamp.elapsed() < Duration::from_millis(800) {
                            *supp_guard = None;
                            return Ok(None);
                        }
                    }
                }

                // 检索已有历史记录执行图片 Bump-to-Top 策略（支持重复复制图片的置顶时间戳刷新）
                if let Some(existing) = self.storage.find_by_content(&hash)? {
                    let bumped = self.storage.bump_to_top(existing.id)?;
                    let _ = self.storage.prune_lru(self.max_capacity);
                    let _ = self.prune_orphan_blobs();
                    self.queue_manager.push(QueueItem {
                        id: bumped.id,
                        content: bumped.content.clone(),
                        entry_type: bumped.entry_type.clone(),
                    });
                    return Ok(Some(bumped));
                }

                // 持久化图片 Blob 到磁盘文件
                let blob_path = self.blob_dir.join(format!("{hash}.bmp"));
                if !blob_path.exists() {
                    let _ = std::fs::write(&blob_path, &image_data);
                }

                // 写入图片元数据
                let entry = self.storage.insert_image(&hash, "", "", "")?;

                // 触发 LRU 淘汰与孤立 Blob GC
                let _ = self.storage.prune_lru(self.max_capacity);
                let _ = self.prune_orphan_blobs();

                self.queue_manager.push(QueueItem {
                    id: entry.id,
                    content: entry.content.clone(),
                    entry_type: entry.entry_type.clone(),
                });
                return Ok(Some(entry));
            }
        }

        // 3. 从底层驱动安全读取当前剪贴板文本
        let current_text = match self.driver.read_text()? {
            Some(t) if !t.trim().is_empty() => t,
            _ => return Ok(None),
        };

        // 4. 回填防环检验：若处于主动回填的 800ms 抑制窗口内且内容匹配，则予以旁路
        {
            let mut suppression_guard = self.paste_suppression.lock().unwrap();
            if let Some((ref text, timestamp)) = *suppression_guard {
                if text == &current_text && timestamp.elapsed() < Duration::from_millis(800) {
                    *suppression_guard = None;
                    return Ok(None);
                }
            }
        }

        // 5. 检索已有历史记录执行 Bump-to-Top 策略（支持连续相同文本的置顶时间戳刷新）
        if let Some(existing) = self.storage.find_by_content(&current_text)? {
            let bumped = self.storage.bump_to_top(existing.id)?;
            let _ = self.storage.prune_lru(self.max_capacity);
            let _ = self.prune_orphan_blobs();

            let mut last_guard = self.last_captured_text.lock().unwrap();
            *last_guard = Some(current_text);
            self.queue_manager.push(QueueItem {
                id: bumped.id,
                content: bumped.content.clone(),
                entry_type: bumped.entry_type.clone(),
            });
            return Ok(Some(bumped));
        }

        // 5. 超大文本 Payload Guard 熔断保护：>2MB 文本仅截取前 200KB 参与全文检索索引
        let index_text = if current_text.len() > LARGE_PAYLOAD_THRESHOLD_BYTES {
            let mut end = MAX_INDEX_PAYLOAD_BYTES;
            while end > 0 && !current_text.is_char_boundary(end) {
                end -= 1;
            }
            &current_text[..end]
        } else {
            &current_text[..]
        };

        // 6. 拼音索引提取：限制最大字符数，毫秒级响应
        let pinyin_source = if index_text.chars().count() > MAX_PINYIN_SOURCE_CHARS {
            let mut end_idx = 0;
            for (i, (byte_idx, _)) in index_text.char_indices().enumerate() {
                if i >= MAX_PINYIN_SOURCE_CHARS {
                    end_idx = byte_idx;
                    break;
                }
            }
            if end_idx > 0 {
                &index_text[..end_idx]
            } else {
                index_text
            }
        } else {
            index_text
        };

        let pinyin_first = PinyinMatcher::to_first_letters_index(pinyin_source);
        let pinyin_full = PinyinMatcher::to_full_pinyin_index(pinyin_source);

        // 7. 全量持久化原始文本，检索列使用截断索引文本
        let entry = self.storage.insert_text(&current_text, index_text, &pinyin_first, &pinyin_full)?;

        // 8. 触发 LRU 淘汰清理超容记录并清理孤立图片 Blob
        let _ = self.storage.prune_lru(self.max_capacity);
        let _ = self.prune_orphan_blobs();

        let mut last_guard = self.last_captured_text.lock().unwrap();
        *last_guard = Some(current_text);
        self.queue_manager.push(QueueItem {
            id: entry.id,
            content: entry.content.clone(),
            entry_type: entry.entry_type.clone(),
        });
        Ok(Some(entry))
    }

    /// 获取最近的历史剪贴板条目（置顶优先、创建时间倒序）
    ///
    /// @param limit 最大返回条目数量
    /// @return 历史条目列表
    pub fn get_entries(&self, limit: usize) -> Result<Vec<ClipboardEntry>, EngineError> {
        let entries = self.storage.get_recent_entries(limit)?;
        Ok(entries)
    }

    /// 基于关键词与拼音检索历史剪贴板条目
    ///
    /// @param query 搜索关键词
    /// @param limit 最大返回条目数量
    /// @return 匹配的历史条目列表
    pub fn search_entries(&self, query: &str, limit: usize) -> Result<Vec<ClipboardEntry>, EngineError> {
        let entries = self.storage.search_entries(query, limit)?;
        Ok(entries)
    }

    /// 切换指定条目的置顶固定状态 (Pin / Unpin)
    ///
    /// @param id 目标条目主键 ID
    /// @return 切换后的最新置顶状态
    pub fn toggle_pin(&self, id: i64) -> Result<bool, EngineError> {
        let is_pinned = self.storage.toggle_pin(id)?;
        Ok(is_pinned)
    }

    /// 将任意文本写入系统剪贴板并模拟触发极速回填
    ///
    /// 自动注入 800ms 防环抑制窗口并调用底层驱动发送粘贴按键。
    ///
    /// @param text 待粘贴的目标文本
    pub fn paste_text(&self, text: &str) -> Result<(), EngineError> {
        // 1. 注入回填防环抑制窗口 (800ms)
        {
            let mut suppression_guard = self.paste_suppression.lock().unwrap();
            *suppression_guard = Some((text.to_string(), Instant::now()));
        }

        // 2. 写入系统剪贴板并模拟按键注入
        self.driver.write_text(text)?;
        self.driver.send_paste()?;

        Ok(())
    }

    /// 触发指定条目的极速回填（支持纯文本与位图图片原样回填）
    ///
    /// @param id 目标条目主键 ID
    pub fn paste_entry(&self, id: i64) -> Result<(), EngineError> {
        let entry = self
            .storage
            .get_entry_by_id(id)?
            .ok_or(EngineError::EntryNotFound(id))?;

        if entry.entry_type == "image" {
            // 1. 读取对应图片 Blob 文件
            let blob_path = self.blob_dir.join(format!("{}.bmp", entry.content));
            if !blob_path.exists() {
                return Err(EngineError::Transform("图片 Blob 文件不存在或已被清理".into()));
            }
            let data = std::fs::read(&blob_path)?;

            // 2. 注入图片回填防环抑制窗口 (800ms)
            {
                let mut suppression_guard = self.paste_suppression_image.lock().unwrap();
                *suppression_guard = Some((entry.content.clone(), Instant::now()));
            }

            // 3. 写入系统剪贴板并模拟按键注入
            self.driver.write_image(&data)?;
            self.driver.send_paste()?;
            Ok(())
        } else {
            self.paste_text(&entry.content)
        }
    }

    /// 对指定图片条目执行离线 OCR 识别并将提取出的文本同步写入数据库索引
    ///
    /// @param id 目标图片条目主键 ID
    /// @return 提取出的文字内容
    pub fn ocr_entry(&self, id: i64) -> Result<String, EngineError> {
        let entry = self
            .storage
            .get_entry_by_id(id)?
            .ok_or(EngineError::EntryNotFound(id))?;

        if entry.entry_type != "image" {
            return Ok(entry.content);
        }

        // 1. 读取对应位图数据
        let blob_path = self.blob_dir.join(format!("{}.bmp", entry.content));
        if !blob_path.exists() {
            return Err(EngineError::Transform("图片 Blob 文件不存在".into()));
        }
        let data = std::fs::read(&blob_path)?;

        // 2. 调用平台抽象驱动原生 OCR 提取
        let ocr_text = self.driver.ocr_image(&data)?;

        // 3. 计算拼音首字母简拼与全拼索引并更新数据库
        let pinyin_first = PinyinMatcher::to_first_letters_index(&ocr_text);
        let pinyin_full = PinyinMatcher::to_full_pinyin_index(&ocr_text);
        self.storage
            .update_entry_ocr(id, &ocr_text, &pinyin_first, &pinyin_full)?;
        Ok(ocr_text)
    }

    /// 读取指定哈希的图片 Blob 并转为前端可直接渲染的 Base64 Data URI
    ///
    /// @param hash 图片内容 SHA-256 哈希值
    /// @return "data:image/bmp;base64,..." 格式字符串
    pub fn get_image_base64(&self, hash: &str) -> Result<String, EngineError> {
        let detail = self.get_image_detail(hash)?;
        Ok(detail.data_url)
    }

    /// 读取指定哈希的图片 Blob 详情（包含宽、高、字节大小与 Data URI）
    ///
    /// @param hash 图片内容 SHA-256 哈希值
    /// @return 包含尺寸、字节数与 Base64 编码的 ImageDetail 视图对象
    pub fn get_image_detail(&self, hash: &str) -> Result<ImageDetail, EngineError> {
        let blob_path = self.blob_dir.join(format!("{hash}.bmp"));
        if !blob_path.exists() {
            return Err(EngineError::Transform("图片 Blob 文件不存在".into()));
        }
        let data = std::fs::read(&blob_path)?;
        let (width, height) = if data.len() >= 26 && &data[0..2] == b"BM" {
            let w = i32::from_le_bytes(data[18..22].try_into().unwrap_or_default()).abs();
            let h = i32::from_le_bytes(data[22..26].try_into().unwrap_or_default()).abs();
            (w, h)
        } else {
            (0, 0)
        };
        let file_size = data.len() as u64;
        let encoded = hash::base64_encode(&data);
        Ok(ImageDetail {
            data_url: format!("data:image/bmp;base64,{encoded}"),
            width,
            height,
            file_size,
        })
    }

    /// 纯函数转换指定条目内容（仅校验与计算，不触发剪贴板和窗口操作）
    ///
    /// @param id 目标条目主键 ID
    /// @param action 格式清洗与转换动作类型
    /// @return 转换后的最终文本
    pub fn transform_entry(
        &self,
        id: i64,
        action: TransformAction,
    ) -> Result<String, EngineError> {
        let entry = self
            .storage
            .get_entry_by_id(id)?
            .ok_or(EngineError::EntryNotFound(id))?;

        TextTransformer::transform(&entry.content, action)
            .map_err(|e| EngineError::Transform(e.to_string()))
    }

    /// 转换指定条目内容并执行极速回填
    ///
    /// @param id 目标条目主键 ID
    /// @param action 格式清洗与转换动作类型
    /// @return 转换后的最终文本
    pub fn transform_and_paste_entry(
        &self,
        id: i64,
        action: TransformAction,
    ) -> Result<String, EngineError> {
        let transformed = self.transform_entry(id, action)?;
        self.paste_text(&transformed)?;
        Ok(transformed)
    }

    /// 获取全部常用短语模板
    pub fn get_all_snippets(&self) -> Result<Vec<Snippet>, EngineError> {
        self.storage.get_all_snippets().map_err(EngineError::Storage)
    }

    /// 保存或更新常用短语模板
    ///
    /// @param id 指定 ID 则更新，None 则新建
    /// @param title 短语标题
    /// @param content 短语模板内容
    /// @param shortcut 快捷缩写
    pub fn save_snippet(
        &self,
        id: Option<i64>,
        title: &str,
        content: &str,
        shortcut: &str,
    ) -> Result<Snippet, EngineError> {
        self.storage
            .save_snippet(id, title, content, shortcut)
            .map_err(EngineError::Storage)
    }

    /// 删除指定常用短语模板
    ///
    /// @param id 短语主键 ID
    pub fn delete_snippet(&self, id: i64) -> Result<bool, EngineError> {
        self.storage.delete_snippet(id).map_err(EngineError::Storage)
    }

    /// 搜索常用短语模板
    ///
    /// @param query 搜索关键词或前缀 (如 "/meet")
    pub fn search_snippets(&self, query: &str) -> Result<Vec<Snippet>, EngineError> {
        self.storage.search_snippets(query).map_err(EngineError::Storage)
    }

    /// 渲染并极速回填常用短语模板内容 (AC-2)
    ///
    /// 解析动态占位符（当前时间、日期、剪贴板等）并调用平台驱动回填展开后的真实文本。
    ///
    /// @param id 短语主键 ID
    /// @return 渲染展开后的文本
    pub fn paste_snippet(&self, id: i64) -> Result<String, EngineError> {
        let snippet = self
            .storage
            .get_snippet_by_id(id)?
            .ok_or(EngineError::EntryNotFound(id))?;

        let current_clipboard = self.driver.read_text().ok().flatten();
        let ctx = SnippetContext::now(current_clipboard);
        let rendered = SnippetEngine::render(&snippet.content, &ctx);

        self.paste_text(&rendered)?;
        Ok(rendered)
    }

    /// 获取队列连贴管理器的状态快照
    pub fn get_paste_queue_status(&self) -> QueueStatus {
        self.queue_manager.get_status()
    }

    /// 切换队列连贴模式 (开启/停止)
    pub fn toggle_paste_queue(&self) -> bool {
        self.queue_manager.toggle()
    }

    /// 开启队列连贴模式
    pub fn start_paste_queue(&self) {
        self.queue_manager.start();
    }

    /// 停止队列连贴模式
    pub fn stop_paste_queue(&self) {
        self.queue_manager.stop();
    }

    /// 获取底层队列连贴管理器引用
    pub fn queue_manager(&self) -> &Arc<PasteQueueManager> {
        &self.queue_manager
    }

    /// 从队列头部弹出一项内容并极速回填至外部目标窗口 (FIFO)
    ///
    /// 若出队后队列已空，状态机会自动将 is_active 置为 false 闭环。
    ///
    /// @return 弹出的项；若队列为空则返回 Ok(None)
    pub fn paste_queue_pop(&self) -> Result<Option<QueueItem>, EngineError> {
        let item = match self.queue_manager.pop() {
            Some(i) => i,
            None => return Ok(None),
        };

        if self.paste_entry(item.id).is_err() {
            // 降级防御：若条目已被外部操作删除，若是文本则直接原样写入回填
            if item.entry_type != "image" {
                self.paste_text(&item.content)?;
            }
        }

        Ok(Some(item))
    }

    /// 多选条目合并粘贴 (AC-4)
    ///
    /// 按传入 ID 顺序提取内容并按指定分隔符（默认换行符 "\n"）合并拼接后极速回填至目标前台窗口。
    ///
    /// @param ids 选中的条目主键 ID 数组
    /// @param separator 合并拼接分隔符
    /// @return 最终合并拼接并回填的文本字符串
    pub fn paste_multiple_entries(
        &self,
        ids: &[i64],
        separator: &str,
    ) -> Result<String, EngineError> {
        let mut parts = Vec::new();
        for &id in ids {
            if let Some(entry) = self.storage.get_entry_by_id(id)? {
                if entry.entry_type == "text" {
                    parts.push(entry.content);
                }
            }
        }

        let merged = parts.join(separator);
        if !merged.is_empty() {
            self.paste_text(&merged)?;
        }
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pal::mock::MockPlatformDriver;
    use crate::storage::sqlite::SqliteStorage;

    fn setup_engine() -> (Arc<MockPlatformDriver>, Arc<ClipboardEngine>) {
        let driver = Arc::new(MockPlatformDriver::new());
        let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());
        let engine = Arc::new(ClipboardEngine::new(driver.clone(), storage));
        (driver, engine)
    }

    #[test]
    fn test_engine_bump_to_top_for_consecutive_same_content() {
        let (_driver, engine) = setup_engine();

        // 1. 复制文本 A
        _driver.write_text("Hello Clip").unwrap();
        let first = engine.handle_clipboard_change().unwrap().unwrap();
        let time1 = first.created_at;

        // 2. 连续再次复制文本 A，验证时间戳被更新且置顶在最前 (A -> A 连续去重前置)
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = engine.handle_clipboard_change().unwrap().unwrap();
        assert_eq!(second.id, first.id, "ID 必须保持一致，不可新增行");
        assert!(second.created_at >= time1, "时间戳必须被刷新");

        let list = engine.get_entries(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, first.id);
    }

    #[test]
    fn test_engine_bump_to_top_for_existing_content() {
        let (driver, engine) = setup_engine();

        // 1. 复制内容 A 与内容 B
        driver.write_text("Content A").unwrap();
        let a = engine.handle_clipboard_change().unwrap().unwrap();

        driver.write_text("Content B").unwrap();
        let _b = engine.handle_clipboard_change().unwrap().unwrap();

        // 原本 B 在最前，A 在后
        let list1 = engine.get_entries(10).unwrap();
        assert_eq!(list1[0].content, "Content B");
        assert_eq!(list1[1].content, "Content A");

        // 2. 再次复制内容 A，触发 Bump-to-Top
        std::thread::sleep(std::time::Duration::from_millis(2));
        driver.write_text("Content A").unwrap();
        let bumped = engine.handle_clipboard_change().unwrap().unwrap();
        assert_eq!(bumped.id, a.id, "ID 必须保持不变，严禁创建冗余行");

        // 3. 验证 A 重新跃升至最前，且总条数依然为 2
        let list2 = engine.get_entries(10).unwrap();
        assert_eq!(list2.len(), 2);
        assert_eq!(list2[0].content, "Content A");
        assert_eq!(list2[1].content, "Content B");
    }

    #[test]
    fn test_engine_payload_guard_truncation_boundary() {
        let (driver, engine) = setup_engine();

        // 构造 >2.5MB 超大文本：前缀为特殊前缀，尾部为 200KB 之外的特殊尾缀
        let prefix = "前缀测试内容_AAA ";
        let filler = "填充内容 ".repeat(300000); // 超过 2.5MB
        let suffix = " 尾缀不可被检索_ZZZ";
        let full_text = format!("{prefix}{filler}{suffix}");
        assert!(full_text.len() > LARGE_PAYLOAD_THRESHOLD_BYTES);

        driver.write_text(&full_text).unwrap();
        let entry = engine.handle_clipboard_change().unwrap().unwrap();

        // 1. 完整原文成功落盘入库
        assert_eq!(entry.content.len(), full_text.len());

        // 2. 检索前缀内容成功命中
        let results_prefix = engine.search_entries("前缀测试内容", 10).unwrap();
        assert_eq!(results_prefix.len(), 1);
        assert_eq!(results_prefix[0].id, entry.id);

        // 3. 检索 200KB 截断边界之外的尾缀关键词，验证不可被命中 (FTS 索引平滑截断)
        let results_suffix = engine.search_entries("尾缀不可被检索", 10).unwrap();
        assert_eq!(results_suffix.len(), 0, "截断阈值之外的内容绝不应被索引");

        // 4. 执行回填，验证回填得到的是 100% 完整超大文本原文
        engine.paste_entry(entry.id).unwrap();
        let pasted_text = driver.read_text().unwrap().unwrap();
        assert_eq!(pasted_text.len(), full_text.len());
        assert!(pasted_text.ends_with(suffix));
    }

    #[test]
    fn test_engine_toggle_pin() {
        let (driver, engine) = setup_engine();

        driver.write_text("Regular Item").unwrap();
        let entry = engine.handle_clipboard_change().unwrap().unwrap();

        assert!(!entry.is_pinned);
        let is_pinned = engine.toggle_pin(entry.id).unwrap();
        assert!(is_pinned);

        let list = engine.get_entries(10).unwrap();
        assert!(list[0].is_pinned);
    }

    #[test]
    fn test_engine_privacy_filter_protocol_flag() {
        let (driver, engine) = setup_engine();

        // 模拟外部密码管理器设置了 Clipboard Viewer Ignore 隐私排除标记
        driver.simulate_privacy_flag(true);
        driver.write_text("SuperSecretPassword123").unwrap();

        let result = engine.handle_clipboard_change().unwrap();
        assert!(result.is_none(), "携带隐私排除标记时必须丢弃");

        // 校验存储层零记录
        let list = engine.get_entries(10).unwrap();
        assert_eq!(list.len(), 0, "SQLite 绝不可产生任何记录");
    }

    #[test]
    fn test_engine_privacy_filter_blacklist_process() {
        let (driver, engine) = setup_engine();

        // 模拟来自黑名单进程 KeePass 的复制
        driver.simulate_source_process(Some("KeePass.exe"));
        driver.write_text("MasterKeyFromKeePass").unwrap();

        let result = engine.handle_clipboard_change().unwrap();
        assert!(result.is_none(), "来自黑名单进程的复制必须被旁路忽略");

        // 校验存储层零记录
        let list = engine.get_entries(10).unwrap();
        assert_eq!(list.len(), 0, "SQLite 绝不可产生任何记录");

        // 切换为安全正常进程 (例如 code.exe)，验证正常捕获
        driver.simulate_source_process(Some("code.exe"));
        driver.write_text("Normal Code Snippet").unwrap();
        let normal = engine.handle_clipboard_change().unwrap().unwrap();
        assert_eq!(normal.content, "Normal Code Snippet");

        let list_after = engine.get_entries(10).unwrap();
        assert_eq!(list_after.len(), 1);
    }

    #[test]
    fn test_engine_transform_and_paste_entry() {
        let (driver, engine) = setup_engine();

        // 1. 入库原始文本
        driver.write_text("hello_world_variable").unwrap();
        let entry = engine.handle_clipboard_change().unwrap().unwrap();

        // 2. 转换成 CamelCase 并回填
        let res = engine
            .transform_and_paste_entry(entry.id, TransformAction::CamelCase)
            .unwrap();
        assert_eq!(res, "helloWorldVariable");

        // 验证 Mock 平台驱动收到了转换后的文本并触发了粘贴
        let current_clipboard = driver.read_text().unwrap().unwrap();
        assert_eq!(current_clipboard, "helloWorldVariable");
        assert_eq!(driver.paste_count(), 1);

        // 3. 转换成 UPPERCASE 并回填
        let res_upper = engine
            .transform_and_paste_entry(entry.id, TransformAction::Uppercase)
            .unwrap();
        assert_eq!(res_upper, "HELLO_WORLD_VARIABLE");
        let current_clipboard_upper = driver.read_text().unwrap().unwrap();
        assert_eq!(current_clipboard_upper, "HELLO_WORLD_VARIABLE");
        assert_eq!(driver.paste_count(), 2);
    }

    fn create_dummy_bmp(width: i32, height: i32, color_byte: u8) -> Vec<u8> {
        let mut bmp = Vec::new();
        bmp.extend_from_slice(b"BM");
        let file_size: u32 = 54 + (width * height * 3) as u32;
        bmp.extend_from_slice(&file_size.to_le_bytes());
        bmp.extend_from_slice(&[0u8; 4]);
        let offset: u32 = 54;
        bmp.extend_from_slice(&offset.to_le_bytes());
        // DIB Header (40 bytes)
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&width.to_le_bytes());
        bmp.extend_from_slice(&height.to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        let image_size = (width * height * 3) as u32;
        bmp.extend_from_slice(&image_size.to_le_bytes());
        bmp.extend_from_slice(&2835i32.to_le_bytes());
        bmp.extend_from_slice(&2835i32.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.resize(54 + (width * height * 3) as usize, color_byte);
        bmp
    }

    #[test]
    fn test_engine_image_capture_bump_and_paste() {
        let driver = Arc::new(MockPlatformDriver::new());
        let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());
        let test_blob_dir = std::env::temp_dir().join(format!(
            "clip_test_blobs_capture_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let engine = Arc::new(
            ClipboardEngine::new(driver.clone(), storage)
                .with_blob_dir(test_blob_dir.clone()),
        );

        // 1. 模拟复制有效图片
        let bmp = create_dummy_bmp(120, 80, 0xAA);
        driver.simulate_clipboard_image_change(Some(bmp.clone()));
        let entry = engine.handle_clipboard_change().unwrap().unwrap();

        assert_eq!(entry.entry_type, "image");
        let detail = engine.get_image_detail(&entry.content).unwrap();
        assert_eq!(detail.width, 120);
        assert_eq!(detail.height, 80);
        assert_eq!(detail.file_size, bmp.len() as u64);

        // 验证文件已落盘
        let blob_file = test_blob_dir.join(format!("{}.bmp", entry.content));
        assert!(blob_file.exists());

        // 验证 Base64 提取
        let b64 = engine.get_image_base64(&entry.content).unwrap();
        assert!(b64.starts_with("data:image/bmp;base64,"));

        // 2. 模拟重复复制同一图片，触发 Bump-to-top
        let bumped = engine.handle_clipboard_change().unwrap().unwrap();
        assert_eq!(bumped.id, entry.id);

        // 3. 执行图片回填
        engine.paste_entry(entry.id).unwrap();
        assert_eq!(driver.last_written_image(), Some(bmp));
        assert_eq!(driver.paste_count(), 1);

        // 清理测试目录
        let _ = std::fs::remove_dir_all(&test_blob_dir);
    }

    #[test]
    fn test_engine_image_ocr_and_search() {
        let driver = Arc::new(MockPlatformDriver::new());
        let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());
        let test_blob_dir = std::env::temp_dir().join(format!(
            "clip_test_blobs_ocr_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let engine = Arc::new(
            ClipboardEngine::new(driver.clone(), storage)
                .with_blob_dir(test_blob_dir.clone()),
        );

        // 1. 捕获图片
        let bmp = create_dummy_bmp(64, 64, 0xBB);
        driver.simulate_clipboard_image_change(Some(bmp));
        let entry = engine.handle_clipboard_change().unwrap().unwrap();

        // 2. 模拟原生离线 OCR 识别
        driver.simulate_ocr_result(Some("重要凭据密码 2026-TOKEN-XYZ"));
        let ocr_res = engine.ocr_entry(entry.id).unwrap();
        assert_eq!(ocr_res, "重要凭据密码 2026-TOKEN-XYZ");

        // 3. 通过 OCR 提取的内容进行关键词与拼音检索
        let search_by_keyword = engine.search_entries("TOKEN-XYZ", 10).unwrap();
        assert_eq!(search_by_keyword.len(), 1);
        assert_eq!(search_by_keyword[0].id, entry.id);

        let search_by_pinyin = engine.search_entries("zypj", 10).unwrap();
        assert_eq!(search_by_pinyin.len(), 1);
        assert_eq!(search_by_pinyin[0].id, entry.id);

        let _ = std::fs::remove_dir_all(&test_blob_dir);
    }

    #[test]
    fn test_engine_image_blob_gc_on_lru() {
        let driver = Arc::new(MockPlatformDriver::new());
        let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());
        let test_blob_dir = std::env::temp_dir().join(format!(
            "clip_test_blobs_gc_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let engine = Arc::new(
            ClipboardEngine::new(driver.clone(), storage)
                .with_capacity(2)
                .with_blob_dir(test_blob_dir.clone()),
        );

        // 连续推入 3 张不同图片
        let bmp1 = create_dummy_bmp(10, 10, 0x01);
        driver.simulate_clipboard_image_change(Some(bmp1));
        let entry1 = engine.handle_clipboard_change().unwrap().unwrap();

        let bmp2 = create_dummy_bmp(20, 20, 0x02);
        driver.simulate_clipboard_image_change(Some(bmp2));
        let entry2 = engine.handle_clipboard_change().unwrap().unwrap();

        let bmp3 = create_dummy_bmp(30, 30, 0x03);
        driver.simulate_clipboard_image_change(Some(bmp3));
        let entry3 = engine.handle_clipboard_change().unwrap().unwrap();

        // entry1 已被淘汰，其对应的 Blob 应该被 GC 清理删除
        let blob1 = test_blob_dir.join(format!("{}.bmp", entry1.content));
        let blob2 = test_blob_dir.join(format!("{}.bmp", entry2.content));
        let blob3 = test_blob_dir.join(format!("{}.bmp", entry3.content));

        assert!(!blob1.exists(), "被淘汰条目的 Blob 文件必须已被 GC 清除");
        assert!(blob2.exists(), "存活条目的 Blob 文件必须保留");
        assert!(blob3.exists(), "存活条目的 Blob 文件必须保留");

        let _ = std::fs::remove_dir_all(&test_blob_dir);
    }

    #[test]
    fn test_engine_paste_snippet_rendering_and_playback() {
        let (driver, engine) = setup_engine();

        // 1. 设置剪贴板现有文本
        driver.write_text("https://github.com/atengk/clip").unwrap();

        // 2. 创建短语模板，包含动态占位符
        let snippet = engine
            .save_snippet(
                None,
                "链接引用",
                "参考链接: {clipboard}\n当前年份: {year}",
                "ref",
            )
            .unwrap();

        // 3. 触发回填短语
        let rendered = engine.paste_snippet(snippet.id).unwrap();
        assert!(rendered.contains("参考链接: https://github.com/atengk/clip"));
        assert!(rendered.contains("当前年份:"));

        // 4. 验证驱动接收到展开后的真实文本并触发了模拟粘贴
        let final_text = driver.read_text().unwrap().unwrap();
        assert_eq!(final_text, rendered);
        assert_eq!(driver.paste_count(), 1);
    }

    #[test]
    fn test_engine_paste_queue_fifo_lifecycle() {
        let (driver, engine) = setup_engine();

        // 1. 开启连贴模式
        assert!(!engine.get_paste_queue_status().is_active);
        let toggled = engine.toggle_paste_queue();
        assert!(toggled);
        assert!(engine.get_paste_queue_status().is_active);

        // 2. 连续复制三段文本 A, B, C
        driver.simulate_clipboard_change(Some("First Data".into()));
        let _ = engine.handle_clipboard_change().unwrap();

        driver.simulate_clipboard_change(Some("Second Data".into()));
        let _ = engine.handle_clipboard_change().unwrap();

        driver.simulate_clipboard_change(Some("Third Data".into()));
        let _ = engine.handle_clipboard_change().unwrap();

        let status = engine.get_paste_queue_status();
        assert_eq!(status.count, 3);
        assert!(status.is_active);

        // 3. 模拟在目标窗口连按回填 (FIFO 顺序)
        let pop1 = engine.paste_queue_pop().unwrap().unwrap();
        assert_eq!(pop1.content, "First Data");
        assert_eq!(driver.read_text().unwrap().unwrap(), "First Data");
        assert_eq!(driver.paste_count(), 1);
        assert_eq!(engine.get_paste_queue_status().count, 2);
        assert!(engine.get_paste_queue_status().is_active);

        let pop2 = engine.paste_queue_pop().unwrap().unwrap();
        assert_eq!(pop2.content, "Second Data");
        assert_eq!(driver.read_text().unwrap().unwrap(), "Second Data");
        assert_eq!(driver.paste_count(), 2);
        assert_eq!(engine.get_paste_queue_status().count, 1);
        assert!(engine.get_paste_queue_status().is_active);

        let pop3 = engine.paste_queue_pop().unwrap().unwrap();
        assert_eq!(pop3.content, "Third Data");
        assert_eq!(driver.read_text().unwrap().unwrap(), "Third Data");
        assert_eq!(driver.paste_count(), 3);

        // 4. 全部出队后，队列清空且模式自动退出 (AC-3)
        let final_status = engine.get_paste_queue_status();
        assert_eq!(final_status.count, 0);
        assert!(!final_status.is_active, "全部出队后连贴模式必须自动销毁闭环");
        assert!(engine.paste_queue_pop().unwrap().is_none());
    }

    #[test]
    fn test_engine_paste_multiple_entries() {
        let (driver, engine) = setup_engine();

        driver.simulate_clipboard_change(Some("Line One".into()));
        let e1 = engine.handle_clipboard_change().unwrap().unwrap();

        driver.simulate_clipboard_change(Some("Line Two".into()));
        let e2 = engine.handle_clipboard_change().unwrap().unwrap();

        driver.simulate_clipboard_change(Some("Line Three".into()));
        let e3 = engine.handle_clipboard_change().unwrap().unwrap();

        // 验证多选以换行符合并粘贴
        let merged = engine
            .paste_multiple_entries(&[e1.id, e2.id, e3.id], "\n")
            .unwrap();
        assert_eq!(merged, "Line One\nLine Two\nLine Three");
        assert_eq!(driver.read_text().unwrap().unwrap(), "Line One\nLine Two\nLine Three");
        assert_eq!(driver.paste_count(), 1);

        // 验证自定义逗号分隔符合并
        let merged_comma = engine
            .paste_multiple_entries(&[e3.id, e1.id], ", ")
            .unwrap();
        assert_eq!(merged_comma, "Line Three, Line One");
        assert_eq!(driver.read_text().unwrap().unwrap(), "Line Three, Line One");
        assert_eq!(driver.paste_count(), 2);
    }
}
