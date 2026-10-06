//! 剪贴板核心业务状态机引擎 (ClipboardEngine)。
//!
//! 负责剪贴板内容捕获、时间窗口回填防环、连续去重以及历史查询与回填编排。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::pal::{PalError, PlatformDriver};
use crate::storage::{ClipboardEntry, Storage, StorageError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

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
        }
    }

    /// 处理剪贴板变更事件
    ///
    /// 当操作系统广播剪贴板更新时触发，负责读取、防环过滤与持久化入库。
    ///
    /// @return 若成功捕获并入库新条目返回 Ok(Some(entry))，若为空或自回填/重复则返回 Ok(None)
    pub fn handle_clipboard_change(&self) -> Result<Option<ClipboardEntry>, EngineError> {
        // 1. 从底层驱动安全读取当前剪贴板文本
        let current_text = match self.driver.read_text()? {
            Some(t) if !t.trim().is_empty() => t,
            _ => return Ok(None),
        };

        // 2. 回填防环检验：若处于主动回填的 800ms 抑制窗口内且内容匹配，则予以旁路
        {
            let mut suppression_guard = self.paste_suppression.lock().unwrap();
            if let Some((ref text, timestamp)) = *suppression_guard {
                if text == &current_text && timestamp.elapsed() < Duration::from_millis(800) {
                    *suppression_guard = None;
                    return Ok(None);
                }
            }
        }

        // 3. 连续重复比对
        {
            let mut last_guard = self.last_captured_text.lock().unwrap();
            if let Some(ref last) = *last_guard {
                if last == &current_text {
                    return Ok(None);
                }
            }
            *last_guard = Some(current_text.clone());
        }

        // 4. 持久化存储至 SQLite 并返回新实体
        let entry = self.storage.insert_text(&current_text)?;
        Ok(Some(entry))
    }

    /// 获取最近的历史剪贴板条目
    ///
    /// @param limit 最大返回条目数量
    /// @return 历史条目列表
    pub fn get_entries(&self, limit: usize) -> Result<Vec<ClipboardEntry>, EngineError> {
        let entries = self.storage.get_recent_entries(limit)?;
        Ok(entries)
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
    fn test_engine_capture_and_deduplicate_consecutive() {
        let (driver, engine) = setup_engine();

        // 1. 模拟复制 "text A"
        driver.write_text("Hello World").unwrap();
        let captured = engine.handle_clipboard_change().unwrap();
        assert!(captured.is_some());
        assert_eq!(captured.unwrap().content, "Hello World");

        // 2. 重复触发相同内容，预期被防环去重忽略
        let duplicate = engine.handle_clipboard_change().unwrap();
        assert!(duplicate.is_none());

        // 3. 复制新文本 "text B"，预期成功入库
        driver.write_text("New Clip Content").unwrap();
        let second_captured = engine.handle_clipboard_change().unwrap();
        assert!(second_captured.is_some());
        assert_eq!(second_captured.unwrap().content, "New Clip Content");

        // 4. 验证历史列表按倒序返回
        let history = engine.get_entries(10).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "New Clip Content");
        assert_eq!(history[1].content, "Hello World");
    }

    #[test]
    fn test_engine_paste_entry_prevents_re_capture_loop() {
        let (driver, engine) = setup_engine();

        // 写入并捕获条目
        driver.write_text("Item for paste").unwrap();
        let entry = engine.handle_clipboard_change().unwrap().unwrap();

        // 回填条目
        assert_eq!(driver.paste_count(), 0);
        engine.paste_entry(entry.id).unwrap();
        assert_eq!(driver.paste_count(), 1);

        // 模拟 Windows 系统因刚才的 write_text 发出 WM_CLIPBOARDUPDATE
        let re_captured = engine.handle_clipboard_change().unwrap();
        assert!(
            re_captured.is_none(),
            "回填自身写入的数据必须被抑制窗口机制拦截，严禁重复入库"
        );
    }
}
