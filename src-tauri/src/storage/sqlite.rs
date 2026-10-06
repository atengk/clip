//! SQLite 存储层实现，提供剪贴板历史条目的持久化存储与倒序查询。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::storage::{ClipboardEntry, Storage, StorageError};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// 基于 rusqlite 的本地 SQLite 存储引擎
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    /// 基于内存数据库创建实例（专门用于无头单元测试毫秒级验证）
    pub fn new_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| StorageError::DatabaseError(format!("打开内存 SQLite 失败: {e}")))?;
        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.init_tables()?;
        Ok(storage)
    }

    /// 基于本地文件系统路径初始化 SQLite 数据库
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let conn = Connection::open(path)
            .map_err(|e| StorageError::DatabaseError(format!("打开文件 SQLite 失败: {e}")))?;
        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.init_tables()?;
        Ok(storage)
    }

    /// 初始化数据表与索引结构
    fn init_tables(&self) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS clipboard_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                entry_type TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                is_pinned INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_entries_created_at ON clipboard_entries(created_at DESC, id DESC);
            "#,
        )
        .map_err(|e| StorageError::DatabaseError(format!("初始化表结构失败: {e}")))?;
        Ok(())
    }
}

impl Storage for SqliteStorage {
    fn insert_text(&self, text: &str) -> Result<ClipboardEntry, StorageError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clipboard_entries (content, entry_type, created_at, is_pinned) VALUES (?1, ?2, ?3, ?4)",
            params![text, "text", now_ms, 0],
        )
        .map_err(|e| StorageError::DatabaseError(format!("写入剪贴板条目失败: {e}")))?;

        let id = conn.last_insert_rowid();

        Ok(ClipboardEntry {
            id,
            content: text.to_string(),
            entry_type: "text".to_string(),
            created_at: now_ms,
            is_pinned: false,
        })
    }

    fn get_recent_entries(&self, limit: usize) -> Result<Vec<ClipboardEntry>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, content, entry_type, created_at, is_pinned FROM clipboard_entries ORDER BY created_at DESC, id DESC LIMIT ?1",
            )
            .map_err(|e| StorageError::DatabaseError(format!("预编译查询失败: {e}")))?;

        let rows = stmt
            .query_map(params![limit as i64], parse_entry_row)
            .map_err(|e| StorageError::DatabaseError(format!("执行查询失败: {e}")))?;

        let mut entries = Vec::new();
        for row in rows {
            let entry = row.map_err(|e| StorageError::DatabaseError(format!("解析条目记录失败: {e}")))?;
            entries.push(entry);
        }

        Ok(entries)
    }

    fn get_entry_by_id(&self, id: i64) -> Result<Option<ClipboardEntry>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, content, entry_type, created_at, is_pinned FROM clipboard_entries WHERE id = ?1",
            )
            .map_err(|e| StorageError::DatabaseError(format!("预编译按 ID 查询失败: {e}")))?;

        let mut rows = stmt
            .query_map(params![id], parse_entry_row)
            .map_err(|e| StorageError::DatabaseError(format!("执行按 ID 查询失败: {e}")))?;

        if let Some(row) = rows.next() {
            let entry = row.map_err(|e| StorageError::DatabaseError(format!("解析记录失败: {e}")))?;
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }
}

/// 解析 SQLite 数据行为剪贴板实体对象
fn parse_entry_row(row: &rusqlite::Row) -> rusqlite::Result<ClipboardEntry> {
    Ok(ClipboardEntry {
        id: row.get(0)?,
        content: row.get(1)?,
        entry_type: row.get(2)?,
        created_at: row.get(3)?,
        is_pinned: row.get::<_, i64>(4)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_storage_empty_returns_empty_list() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let entries = storage.get_recent_entries(10).unwrap();
        assert!(entries.is_empty(), "查询空集合时必须返回空数组");
    }

    #[test]
    fn test_sqlite_storage_insert_and_get_recent() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let entry1 = storage.insert_text("First Clip").unwrap();
        assert_eq!(entry1.content, "First Clip");
        assert_eq!(entry1.entry_type, "text");
        assert!(!entry1.is_pinned);

        let entry2 = storage.insert_text("Second Clip").unwrap();
        assert_eq!(entry2.content, "Second Clip");

        let entries = storage.get_recent_entries(10).unwrap();
        assert_eq!(entries.len(), 2);
        // 倒序排列：最新的在最前
        assert_eq!(entries[0].content, "Second Clip");
        assert_eq!(entries[1].content, "First Clip");
    }

    #[test]
    fn test_sqlite_storage_get_entry_by_id() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let inserted = storage.insert_text("Target Item").unwrap();

        let found = storage.get_entry_by_id(inserted.id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().content, "Target Item");

        let not_found = storage.get_entry_by_id(9999).unwrap();
        assert!(not_found.is_none());
    }
}
