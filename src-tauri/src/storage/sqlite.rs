//! SQLite 存储层实现，提供剪贴板历史条目的持久化存储、FTS5 全文索引同步与拼音/多词检索。
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

    /// 初始化数据表、FTS5 全文索引与触发器 (AC-5)
    fn init_tables(&self) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            -- 1. 主业务存储表与多列索引
            CREATE TABLE IF NOT EXISTS clipboard_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                entry_type TEXT NOT NULL,
                pinyin_first TEXT NOT NULL DEFAULT '',
                pinyin_full TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                is_pinned INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_entries_created_at ON clipboard_entries(created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_entries_pinyin_first ON clipboard_entries(pinyin_first);

            -- 2. FTS5 全文检索引擎与同步触发器
            CREATE VIRTUAL TABLE IF NOT EXISTS clipboard_entries_fts USING fts5(
                content,
                pinyin_first,
                pinyin_full,
                content='clipboard_entries',
                content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS trg_entries_ai AFTER INSERT ON clipboard_entries BEGIN
                INSERT INTO clipboard_entries_fts(rowid, content, pinyin_first, pinyin_full)
                VALUES (new.id, new.content, new.pinyin_first, new.pinyin_full);
            END;

            CREATE TRIGGER IF NOT EXISTS trg_entries_ad AFTER DELETE ON clipboard_entries BEGIN
                INSERT INTO clipboard_entries_fts(clipboard_entries_fts, rowid, content, pinyin_first, pinyin_full)
                VALUES ('delete', old.id, old.content, old.pinyin_first, old.pinyin_full);
            END;

            CREATE TRIGGER IF NOT EXISTS trg_entries_au AFTER UPDATE ON clipboard_entries BEGIN
                INSERT INTO clipboard_entries_fts(clipboard_entries_fts, rowid, content, pinyin_first, pinyin_full)
                VALUES ('delete', old.id, old.content, old.pinyin_first, old.pinyin_full);
                INSERT INTO clipboard_entries_fts(rowid, content, pinyin_first, pinyin_full)
                VALUES (new.id, new.content, new.pinyin_first, new.pinyin_full);
            END;
            "#,
        )
        .map_err(|e| StorageError::DatabaseError(format!("初始化数据表与 FTS5 触发器失败: {e}")))?;
        Ok(())
    }
}

impl Storage for SqliteStorage {
    fn insert_text(
        &self,
        text: &str,
        pinyin_first: &str,
        pinyin_full: &str,
    ) -> Result<ClipboardEntry, StorageError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clipboard_entries (content, entry_type, pinyin_first, pinyin_full, created_at, is_pinned) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![text, "text", pinyin_first, pinyin_full, now_ms, 0],
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

    fn search_entries(&self, query: &str, limit: usize) -> Result<Vec<ClipboardEntry>, StorageError> {
        let words: Vec<&str> = query.split_whitespace().collect();
        if words.is_empty() {
            return self.get_recent_entries(limit);
        }

        // 1. 动态构造多关键词 AND 匹配 SQL，同时支持内容原字与拼音简拼/全拼检索
        let mut conditions = Vec::with_capacity(words.len());
        let mut sql_params: Vec<String> = Vec::with_capacity(words.len() * 3);

        for word in &words {
            let pattern = format!("%{}%", word.to_lowercase());
            conditions.push("(lower(content) LIKE ? OR lower(pinyin_first) LIKE ? OR lower(pinyin_full) LIKE ?)");
            sql_params.push(pattern.clone());
            sql_params.push(pattern.clone());
            sql_params.push(pattern);
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT id, content, entry_type, created_at, is_pinned FROM clipboard_entries WHERE {} ORDER BY created_at DESC, id DESC LIMIT {}",
            where_clause, limit
        );

        // 2. 执行安全参数绑定查询
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| StorageError::DatabaseError(format!("预编译搜索 SQL 失败: {e}")))?;

        let rusqlite_params = rusqlite::params_from_iter(sql_params.iter());
        let rows = stmt
            .query_map(rusqlite_params, parse_entry_row)
            .map_err(|e| StorageError::DatabaseError(format!("执行搜索失败: {e}")))?;

        let mut entries = Vec::new();
        for row in rows {
            let entry = row.map_err(|e| StorageError::DatabaseError(format!("解析搜索记录失败: {e}")))?;
            entries.push(entry);
        }

        Ok(entries)
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
        let entry1 = storage.insert_text("First Clip", "first clip", "first clip").unwrap();
        assert_eq!(entry1.content, "First Clip");
        assert_eq!(entry1.entry_type, "text");
        assert!(!entry1.is_pinned);

        let entry2 = storage.insert_text("Second Clip", "second clip", "second clip").unwrap();
        assert_eq!(entry2.content, "Second Clip");

        let entries = storage.get_recent_entries(10).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].content, "Second Clip");
        assert_eq!(entries[1].content, "First Clip");
    }

    #[test]
    fn test_sqlite_fts5_triggers_synchronization() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let entry = storage.insert_text("银行卡号 622202", "yhk", "yinhangka").unwrap();

        // 验证 FTS5 虚拟表通过 AFTER INSERT 触发器自动同步记录
        let conn = storage.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT rowid, content, pinyin_first FROM clipboard_entries_fts WHERE rowid = ?1")
            .unwrap();
        let mut rows = stmt.query(params![entry.id]).unwrap();
        let fts_row = rows.next().unwrap();
        assert!(fts_row.is_some(), "FTS5 触发器必须同步新增记录");
        let row = fts_row.unwrap();
        let rowid: i64 = row.get(0).unwrap();
        let content: String = row.get(1).unwrap();
        assert_eq!(rowid, entry.id);
        assert_eq!(content, "银行卡号 622202");
    }

    #[test]
    fn test_sqlite_search_by_pinyin_first_and_full() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.insert_text("银行卡", "yhk yxk", "yinhangka yinxingka").unwrap();
        storage.insert_text("身份证", "sfz", "shenfenzheng").unwrap();
        storage.insert_text("微信公众号", "wxgzh", "weixingongzhonghao").unwrap();

        // 简拼检索 "yhk" -> 命中 "银行卡"
        let res = storage.search_entries("yhk", 10).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].content, "银行卡");

        // 全拼检索 "weixin" -> 命中 "微信公众号"
        let res_wx = storage.search_entries("weixin", 10).unwrap();
        assert_eq!(res_wx.len(), 1);
        assert_eq!(res_wx[0].content, "微信公众号");
    }

    #[test]
    fn test_sqlite_search_multi_words_and() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.insert_text("2026年微信API接口规范", "2026nwxapijk", "2026nianweixinapijiekou").unwrap();
        storage.insert_text("2026年年度财务报告", "2026nndcwbg", "2026niannianducaiwubaogao").unwrap();
        storage.insert_text("API设计指导原则", "apisj zdyz", "apishejizhidaoyuanze").unwrap();

        // 多词空格分割 AND 匹配: "2026 api" 仅能匹配同时满足的条目
        let res = storage.search_entries("2026 api", 10).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].content, "2026年微信API接口规范");
    }
}
