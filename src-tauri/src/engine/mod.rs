//! 剪贴板核心业务状态机引擎 (ClipboardEngine)。
//!
//! 负责剪贴板内容捕获、时间窗口回填防环、Bump-to-Top 策略、置顶管理与超大文本 Payload Guard 熔断保护。
//!
//! @author Ateng
//! @since 2026-10-06

pub mod pinyin;
pub mod privacy;

use crate::engine::pinyin::PinyinMatcher;
use crate::engine::privacy::PrivacyFilter;
use crate::pal::{PalError, PlatformDriver};
use crate::storage::{ClipboardEntry, Storage, StorageError};
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
    #[error("目标条目未找到: {0}")]
    EntryNotFound(i64),
}

/// 剪贴板状态机核心引擎
pub struct ClipboardEngine {
    driver: Arc<dyn PlatformDriver>,
    storage: Arc<dyn Storage>,
    last_captured_text: Mutex<Option<String>>,
    paste_suppression: Mutex<Option<(String, Instant)>>,
    max_capacity: usize,
    privacy_filter: PrivacyFilter,
}

impl ClipboardEngine {
    /// 构造全新的核心业务引擎
    ///
    /// @param driver 平台抽象驱动实例
    /// @param storage 存储层实例
    pub fn new(driver: Arc<dyn PlatformDriver>, storage: Arc<dyn Storage>) -> Self {
        Self {
            driver,
            storage,
            last_captured_text: Mutex::new(None),
            paste_suppression: Mutex::new(None),
            max_capacity: DEFAULT_MAX_CAPACITY,
            privacy_filter: PrivacyFilter::new(),
        }
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

        // 2. 从底层驱动安全读取当前剪贴板文本
        let current_text = match self.driver.read_text()? {
            Some(t) if !t.trim().is_empty() => t,
            _ => return Ok(None),
        };

        // 3. 回填防环检验：若处于主动回填的 800ms 抑制窗口内且内容匹配，则予以旁路
        {
            let mut suppression_guard = self.paste_suppression.lock().unwrap();
            if let Some((ref text, timestamp)) = *suppression_guard {
                if text == &current_text && timestamp.elapsed() < Duration::from_millis(800) {
                    *suppression_guard = None;
                    return Ok(None);
                }
            }
        }

        // 4. 检索已有历史记录执行 Bump-to-Top 策略（支持连续相同文本的置顶时间戳刷新）
        if let Some(existing) = self.storage.find_by_content(&current_text)? {
            let bumped = self.storage.bump_to_top(existing.id)?;
            let _ = self.storage.prune_lru(self.max_capacity);

            let mut last_guard = self.last_captured_text.lock().unwrap();
            *last_guard = Some(current_text);
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

        // 8. 触发 LRU 淘汰清理超容记录
        let _ = self.storage.prune_lru(self.max_capacity);

        let mut last_guard = self.last_captured_text.lock().unwrap();
        *last_guard = Some(current_text);
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

    /// 触发指定条目的极速回填
    ///
    /// 将目标文本注入系统剪贴板，设置回填防环抑制窗口，并通过驱动模拟发送粘贴快捷键。
    ///
    /// @param id 目标条目主键 ID
    pub fn paste_entry(&self, id: i64) -> Result<(), EngineError> {
        // 1. 根据 ID 查询目标条目
        let entry = self
            .storage
            .get_entry_by_id(id)?
            .ok_or(EngineError::EntryNotFound(id))?;

        // 2. 注入回填防环抑制窗口 (800ms)
        {
            let mut suppression_guard = self.paste_suppression.lock().unwrap();
            *suppression_guard = Some((entry.content.clone(), Instant::now()));
        }

        // 3. 写入系统剪贴板并模拟按键注入
        self.driver.write_text(&entry.content)?;
        self.driver.send_paste()?;

        Ok(())
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
}
