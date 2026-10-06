//! 存储层抽象契约与数据模型。
//!
//! @author Ateng
//! @since 2026-10-06

pub mod sqlite;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 存储层统一错误枚举
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("数据库操作异常: {0}")]
    DatabaseError(String),
    #[error("条目未找到: id={0}")]
    NotFound(i64),
}

/// 剪贴板条目实体 (Clipboard Entry)
///
/// 遵循 CONTEXT.md 领域模型定义，包含原始内容、类型、时间戳及固定状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardEntry {
    /// 唯一主键自增 ID
    pub id: i64,
    /// 纯文本载荷内容
    pub content: String,
    /// 条目类型 (纯文本为 "text")
    pub entry_type: String,
    /// 创建毫秒时间戳
    pub created_at: i64,
    /// 是否置顶固定
    pub is_pinned: bool,
}

/// 存储驱动统一契约 (Storage Trait)
pub trait Storage: Send + Sync {
    /// 插入纯文本条目
    ///
    /// @param text 纯文本内容
    /// @return 成功返回持久化后的剪贴板条目对象
    fn insert_text(&self, text: &str) -> Result<ClipboardEntry, StorageError>;

    /// 获取最近历史条目列表（按创建时间倒序）
    ///
    /// @param limit 最大返回条目数
    /// @return 历史条目列表，无匹配数据时返回空集合
    fn get_recent_entries(&self, limit: usize) -> Result<Vec<ClipboardEntry>, StorageError>;

    /// 根据唯一 ID 查询单条记录
    ///
    /// @param id 条目主键 ID
    /// @return 存在返回 Some(ClipboardEntry)，不存在返回 Ok(None)
    fn get_entry_by_id(&self, id: i64) -> Result<Option<ClipboardEntry>, StorageError>;
}
