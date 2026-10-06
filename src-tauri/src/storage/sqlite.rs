//! SQLite 存储层实现，提供剪贴板历史条目的持久化存储、FTS5 全文索引同步、置顶及 LRU 容量淘汰。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::storage::{ClipboardEntry, Snippet, Storage, StorageError};
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

    /// 初始化数据表、多列索引与 FTS5 触发器 (AC-5)
    fn init_tables(&self) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            -- 1. 主业务存储表与多列索引 (排除大文本列的 B-Tree 索引以杜绝写放大)
            CREATE TABLE IF NOT EXISTS clipboard_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                entry_type TEXT NOT NULL,
                fts_content TEXT NOT NULL DEFAULT '',
                pinyin_first TEXT NOT NULL DEFAULT '',
                pinyin_full TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                is_pinned INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_entries_order ON clipboard_entries(is_pinned DESC, created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_entries_pinyin_first ON clipboard_entries(pinyin_first);

            -- 2. FTS5 全文检索引擎与同步触发器 (全文检索对截断后的 fts_content 建立索引)
            CREATE VIRTUAL TABLE IF NOT EXISTS clipboard_entries_fts USING fts5(
                fts_content,
                pinyin_first,
                pinyin_full,
                content='clipboard_entries',
                content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS trg_entries_ai AFTER INSERT ON clipboard_entries BEGIN
                INSERT INTO clipboard_entries_fts(rowid, fts_content, pinyin_first, pinyin_full)
                VALUES (new.id, new.fts_content, new.pinyin_first, new.pinyin_full);
            END;

            CREATE TRIGGER IF NOT EXISTS trg_entries_ad AFTER DELETE ON clipboard_entries BEGIN
                INSERT INTO clipboard_entries_fts(clipboard_entries_fts, rowid, fts_content, pinyin_first, pinyin_full)
                VALUES ('delete', old.id, old.fts_content, old.pinyin_first, old.pinyin_full);
            END;

            CREATE TRIGGER IF NOT EXISTS trg_entries_au AFTER UPDATE ON clipboard_entries BEGIN
                INSERT INTO clipboard_entries_fts(clipboard_entries_fts, rowid, fts_content, pinyin_first, pinyin_full)
                VALUES ('delete', old.id, old.fts_content, old.pinyin_first, old.pinyin_full);
                INSERT INTO clipboard_entries_fts(rowid, fts_content, pinyin_first, pinyin_full)
                VALUES (new.id, new.fts_content, new.pinyin_first, new.pinyin_full);
            END;

            -- 3. 常用短语模板表 (独立持久化，彻底隔离于剪贴板 1000 条上限与 LRU 淘汰)
            CREATE TABLE IF NOT EXISTS snippets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                shortcut TEXT NOT NULL DEFAULT '',
                pinyin_first TEXT NOT NULL DEFAULT '',
                pinyin_full TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_snippets_updated ON snippets(updated_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_snippets_shortcut ON snippets(shortcut);
            CREATE INDEX IF NOT EXISTS idx_snippets_pinyin ON snippets(pinyin_first);

            -- 4. 应用元数据配置表 (杜绝用户清空数据后种子模板被意外重新播种)
            CREATE TABLE IF NOT EXISTS app_metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )
        .map_err(|e| StorageError::DatabaseError(format!("初始化数据表与 FTS5 触发器失败: {e}")))?;

        // 初始化内置开箱即用示例短语模板 (仅在首次初始化时播种一次，用户主动清空后绝不复活)
        let is_seeded: bool = conn
            .query_row(
                "SELECT 1 FROM app_metadata WHERE key = 'snippets_seeded'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if !is_seeded {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            let _ = conn.execute(
                "INSERT INTO snippets (title, content, shortcut, pinyin_first, pinyin_full, created_at, updated_at) VALUES
                 ('今日站会汇报', '【{current_date} 站会汇报】\n1. 昨日进展：\n2. 今日计划：\n3. 阻塞风险：无', 'meet', 'jrzhhb', 'jinrizhanhuihuibao', ?1, ?1),
                 ('当前时间戳', '{datetime}', 'time', 'dqsjc', 'dangqianshijianchuo', ?1, ?1),
                 ('剪贴板引用回复', '> {clipboard}\n\n已收到并处理。', 'quote', 'jtbyyhf', 'jiantiebanyinyonghuifu', ?1, ?1)",
                params![now_ms],
            );
            let _ = conn.execute(
                "INSERT OR REPLACE INTO app_metadata (key, value) VALUES ('snippets_seeded', 'true')",
                [],
            );
        }

        Ok(())
    }
}

impl Storage for SqliteStorage {
    fn insert_text(
        &self,
        text: &str,
        fts_content: &str,
        pinyin_first: &str,
        pinyin_full: &str,
    ) -> Result<ClipboardEntry, StorageError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clipboard_entries (content, entry_type, fts_content, pinyin_first, pinyin_full, created_at, is_pinned) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![text, "text", fts_content, pinyin_first, pinyin_full, now_ms, 0],
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
                "SELECT id, content, entry_type, created_at, is_pinned FROM clipboard_entries ORDER BY is_pinned DESC, created_at DESC, id DESC LIMIT ?1",
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

        // 1. 动态构造多关键词 AND 匹配 SQL，统一按置顶优先排序展示
        let mut conditions = Vec::with_capacity(words.len());
        let mut sql_params: Vec<String> = Vec::with_capacity(words.len() * 3);

        for word in &words {
            let pattern = format!("%{}%", word.to_lowercase());
            conditions.push("(lower(fts_content) LIKE ? OR lower(pinyin_first) LIKE ? OR lower(pinyin_full) LIKE ?)");
            sql_params.push(pattern.clone());
            sql_params.push(pattern.clone());
            sql_params.push(pattern);
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT id, content, entry_type, created_at, is_pinned FROM clipboard_entries WHERE {} ORDER BY is_pinned DESC, created_at DESC, id DESC LIMIT {}",
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

    fn toggle_pin(&self, id: i64) -> Result<bool, StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE clipboard_entries SET is_pinned = 1 - is_pinned WHERE id = ?1",
                params![id],
            )
            .map_err(|e| StorageError::DatabaseError(format!("更新置顶状态失败: {e}")))?;

        if affected == 0 {
            return Err(StorageError::NotFound(id));
        }

        let is_pinned: i64 = conn
            .query_row(
                "SELECT is_pinned FROM clipboard_entries WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| StorageError::DatabaseError(format!("查询最新置顶状态失败: {e}")))?;

        Ok(is_pinned != 0)
    }

    fn find_by_content(&self, text: &str) -> Result<Option<ClipboardEntry>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, content, entry_type, created_at, is_pinned FROM clipboard_entries WHERE content = ?1 LIMIT 1",
            )
            .map_err(|e| StorageError::DatabaseError(format!("预编译按内容查询失败: {e}")))?;

        let mut rows = stmt
            .query_map(params![text], parse_entry_row)
            .map_err(|e| StorageError::DatabaseError(format!("执行内容匹配失败: {e}")))?;

        if let Some(row) = rows.next() {
            let entry = row.map_err(|e| StorageError::DatabaseError(format!("解析条目失败: {e}")))?;
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    fn bump_to_top(&self, id: i64) -> Result<ClipboardEntry, StorageError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        {
            let conn = self.conn.lock().unwrap();
            let affected = conn
                .execute(
                    "UPDATE clipboard_entries SET created_at = ?1 WHERE id = ?2",
                    params![now_ms, id],
                )
                .map_err(|e| StorageError::DatabaseError(format!("执行 Bump-to-Top 失败: {e}")))?;

            if affected == 0 {
                return Err(StorageError::NotFound(id));
            }
        }

        // 复用公共查询方法，消除重复样板代码
        self.get_entry_by_id(id)?
            .ok_or(StorageError::NotFound(id))
    }

    fn prune_lru(&self, max_capacity: usize) -> Result<usize, StorageError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                r#"
                DELETE FROM clipboard_entries
                WHERE is_pinned = 0 AND id NOT IN (
                    SELECT id FROM clipboard_entries
                    WHERE is_pinned = 0
                    ORDER BY created_at DESC, id DESC
                    LIMIT ?1
                )
                "#,
                params![max_capacity as i64],
            )
            .map_err(|e| StorageError::DatabaseError(format!("执行 LRU 容量淘汰失败: {e}")))?;

        Ok(affected)
    }

    fn insert_image(
        &self,
        blob_name: &str,
        ocr_text: &str,
        pinyin_first: &str,
        pinyin_full: &str,
    ) -> Result<ClipboardEntry, StorageError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clipboard_entries (content, entry_type, fts_content, pinyin_first, pinyin_full, created_at, is_pinned)
             VALUES (?1, 'image', ?2, ?3, ?4, ?5, 0)",
            params![blob_name, ocr_text, pinyin_first, pinyin_full, now_ms],
        )
        .map_err(|e| StorageError::DatabaseError(format!("插入图片记录失败: {e}")))?;

        let id = conn.last_insert_rowid();
        Ok(ClipboardEntry {
            id,
            content: blob_name.to_string(),
            entry_type: "image".to_string(),
            created_at: now_ms,
            is_pinned: false,
        })
    }

    fn update_entry_ocr(
        &self,
        id: i64,
        ocr_text: &str,
        pinyin_first: &str,
        pinyin_full: &str,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_entries
             SET fts_content = ?1, pinyin_first = ?2, pinyin_full = ?3
             WHERE id = ?4",
            params![ocr_text, pinyin_first, pinyin_full, id],
        )
        .map_err(|e| StorageError::DatabaseError(format!("更新 OCR 文本失败: {e}")))?;
        Ok(())
    }

    fn get_all_image_contents(&self) -> Result<Vec<String>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT content FROM clipboard_entries WHERE entry_type = 'image'")
            .map_err(|e| StorageError::DatabaseError(format!("准备查询所有图片失败: {e}")))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| StorageError::DatabaseError(format!("执行查询所有图片失败: {e}")))?;

        let list = rows.flatten().collect();
        Ok(list)
    }

    fn save_snippet(
        &self,
        id: Option<i64>,
        title: &str,
        content: &str,
        shortcut: &str,
    ) -> Result<Snippet, StorageError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let clean_shortcut = shortcut.trim().trim_start_matches('/');
        let (py_first, py_full) = extract_snippet_pinyin(title);
        let conn = self.conn.lock().unwrap();

        if let Some(target_id) = id {
            let rows_affected = conn
                .execute(
                    "UPDATE snippets SET title = ?1, content = ?2, shortcut = ?3, pinyin_first = ?4, pinyin_full = ?5, updated_at = ?6 WHERE id = ?7",
                    params![title, content, clean_shortcut, py_first, py_full, now_ms, target_id],
                )
                .map_err(|e| StorageError::DatabaseError(format!("更新常用短语失败: {e}")))?;

            if rows_affected == 0 {
                return Err(StorageError::NotFound(target_id));
            }

            let created_at: i64 = conn
                .query_row(
                    "SELECT created_at FROM snippets WHERE id = ?1",
                    params![target_id],
                    |r| r.get(0),
                )
                .map_err(|e| StorageError::DatabaseError(format!("查询短语创建时间失败: {e}")))?;

            Ok(Snippet {
                id: target_id,
                title: title.to_string(),
                content: content.to_string(),
                shortcut: clean_shortcut.to_string(),
                created_at,
                updated_at: now_ms,
            })
        } else {
            conn.execute(
                "INSERT INTO snippets (title, content, shortcut, pinyin_first, pinyin_full, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![title, content, clean_shortcut, py_first, py_full, now_ms, now_ms],
            )
            .map_err(|e| StorageError::DatabaseError(format!("新建常用短语失败: {e}")))?;

            let new_id = conn.last_insert_rowid();
            Ok(Snippet {
                id: new_id,
                title: title.to_string(),
                content: content.to_string(),
                shortcut: clean_shortcut.to_string(),
                created_at: now_ms,
                updated_at: now_ms,
            })
        }
    }

    fn delete_snippet(&self, id: i64) -> Result<bool, StorageError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute("DELETE FROM snippets WHERE id = ?1", params![id])
            .map_err(|e| StorageError::DatabaseError(format!("删除常用短语失败: {e}")))?;
        Ok(rows > 0)
    }

    fn get_all_snippets(&self) -> Result<Vec<Snippet>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, title, content, shortcut, created_at, updated_at FROM snippets ORDER BY updated_at DESC, id DESC")
            .map_err(|e| StorageError::DatabaseError(format!("准备查询所有短语失败: {e}")))?;

        let rows = stmt
            .query_map([], parse_snippet_row)
            .map_err(|e| StorageError::DatabaseError(format!("执行查询所有短语失败: {e}")))?;

        let list = rows.flatten().collect();
        Ok(list)
    }

    fn get_snippet_by_id(&self, id: i64) -> Result<Option<Snippet>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, title, content, shortcut, created_at, updated_at FROM snippets WHERE id = ?1")
            .map_err(|e| StorageError::DatabaseError(format!("准备按 ID 查询短语失败: {e}")))?;

        let mut rows = stmt
            .query_map(params![id], parse_snippet_row)
            .map_err(|e| StorageError::DatabaseError(format!("执行按 ID 查询短语失败: {e}")))?;

        match rows.next() {
            Some(res) => res
                .map(Some)
                .map_err(|e| StorageError::DatabaseError(format!("解析短语失败: {e}"))),
            None => Ok(None),
        }
    }

    fn search_snippets(&self, query: &str) -> Result<Vec<Snippet>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let trimmed = query.trim();

        if trimmed.starts_with('/') {
            let shortcut_prefix = trimmed.trim_start_matches('/');
            if shortcut_prefix.is_empty() {
                drop(conn);
                return self.get_all_snippets();
            }
            let pattern = format!("%{shortcut_prefix}%");
            let mut stmt = conn
                .prepare(
                    "SELECT id, title, content, shortcut, created_at, updated_at FROM snippets
                     WHERE shortcut LIKE ?1 OR title LIKE ?1 OR pinyin_first LIKE ?1
                     ORDER BY CASE
                         WHEN shortcut = ?2 THEN 0
                         WHEN shortcut LIKE (?2 || '%') THEN 1
                         ELSE 2
                     END, updated_at DESC, id DESC",
                )
                .map_err(|e| StorageError::DatabaseError(format!("准备搜索短语失败: {e}")))?;

            let rows = stmt
                .query_map(params![pattern, shortcut_prefix], parse_snippet_row)
                .map_err(|e| StorageError::DatabaseError(format!("执行搜索短语失败: {e}")))?;

            let list = rows.flatten().collect();
            Ok(list)
        } else {
            let pattern = format!("%{trimmed}%");
            let mut stmt = conn
                .prepare(
                    "SELECT id, title, content, shortcut, created_at, updated_at FROM snippets
                     WHERE title LIKE ?1 OR shortcut LIKE ?1 OR content LIKE ?1 OR pinyin_first LIKE ?1 OR pinyin_full LIKE ?1
                     ORDER BY updated_at DESC, id DESC",
                )
                .map_err(|e| StorageError::DatabaseError(format!("准备搜索短语失败: {e}")))?;

            let rows = stmt
                .query_map(params![pattern], parse_snippet_row)
                .map_err(|e| StorageError::DatabaseError(format!("执行搜索短语失败: {e}")))?;

            let list = rows.flatten().collect();
            Ok(list)
        }
    }
}

/// 提取文本的简拼与全拼基础索引 (小写)
fn extract_snippet_pinyin(text: &str) -> (String, String) {
    use pinyin::ToPinyinMulti;
    let mut initials = String::new();
    let mut fulls = String::new();
    for ch in text.chars() {
        if let Some(multi) = ch.to_pinyin_multi() {
            if let Some(p) = multi.into_iter().next() {
                let plain = p.plain();
                if let Some(first) = plain.chars().next() {
                    initials.push(first.to_ascii_lowercase());
                }
                fulls.push_str(&plain.to_ascii_lowercase());
                continue;
            }
        }
        let lower = ch.to_ascii_lowercase();
        initials.push(lower);
        fulls.push(lower);
    }
    (initials, fulls)
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

/// 解析 SQLite 数据行为常用短语实体对象
fn parse_snippet_row(row: &rusqlite::Row) -> rusqlite::Result<Snippet> {
    Ok(Snippet {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        shortcut: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_bump_to_top() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let first = storage.insert_text("First Clip", "First Clip", "first", "first").unwrap();
        let _second = storage.insert_text("Second Clip", "Second Clip", "second", "second").unwrap();

        // 原本 Second 处于最前
        let list1 = storage.get_recent_entries(10).unwrap();
        assert_eq!(list1[0].content, "Second Clip");
        assert_eq!(list1[1].content, "First Clip");

        // 执行 Bump-to-Top，刷新 First 的时间戳
        std::thread::sleep(std::time::Duration::from_millis(2));
        let bumped = storage.bump_to_top(first.id).unwrap();
        assert_eq!(bumped.id, first.id);

        let list2 = storage.get_recent_entries(10).unwrap();
        assert_eq!(list2[0].content, "First Clip");
        assert_eq!(list2[1].content, "Second Clip");
    }

    #[test]
    fn test_sqlite_toggle_pin() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let entry1 = storage.insert_text("Regular Item", "Regular Item", "reg", "reg").unwrap();
        let entry2 = storage.insert_text("Important Item", "Important Item", "imp", "imp").unwrap();

        // 默认 entry2 在最前
        let list1 = storage.get_recent_entries(10).unwrap();
        assert_eq!(list1[0].id, entry2.id);

        // 将老项 entry1 置顶
        let is_pinned = storage.toggle_pin(entry1.id).unwrap();
        assert!(is_pinned);

        // 置顶后 entry1 强制跃升至首位
        let list2 = storage.get_recent_entries(10).unwrap();
        assert_eq!(list2[0].id, entry1.id);
        assert!(list2[0].is_pinned);

        // 取消置顶
        let unpinned = storage.toggle_pin(entry1.id).unwrap();
        assert!(!unpinned);
    }

    #[test]
    fn test_sqlite_lru_prune_exempts_pinned() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let pinned = storage.insert_text("Very Important (Pinned)", "vip", "vip", "vip").unwrap();
        storage.toggle_pin(pinned.id).unwrap();

        // 插入 5 条普通记录
        for i in 1..=5 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            storage.insert_text(&format!("Item {i}"), &format!("Item {i}"), "item", "item").unwrap();
        }

        // 总共 6 条 (1 置顶 + 5 普通)。LRU 设置非置顶上限为 2
        let pruned = storage.prune_lru(2).unwrap();
        assert_eq!(pruned, 3, "应该淘汰 3 条最早的非置顶记录");

        let remaining = storage.get_recent_entries(10).unwrap();
        assert_eq!(remaining.len(), 3);
        // 置顶项永久保留在最前
        assert_eq!(remaining[0].id, pinned.id);
        assert!(remaining[0].is_pinned);
        // 剩下的 2 条为最新的普通项
        assert_eq!(remaining[1].content, "Item 5");
        assert_eq!(remaining[2].content, "Item 4");
    }

    #[test]
    fn test_sqlite_fts5_triggers_synchronization() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let entry = storage.insert_text("银行卡号 622202", "银行卡号 622202", "yhk", "yinhangka").unwrap();

        // 验证 FTS5 虚拟表通过 AFTER INSERT 触发器自动同步记录
        let conn = storage.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT rowid, fts_content, pinyin_first FROM clipboard_entries_fts WHERE rowid = ?1")
            .unwrap();
        let mut rows = stmt.query(params![entry.id]).unwrap();
        let fts_row = rows.next().unwrap();
        assert!(fts_row.is_some(), "FTS5 触发器必须同步新增记录");
        let row = fts_row.unwrap();
        let rowid: i64 = row.get(0).unwrap();
        let fts_content: String = row.get(1).unwrap();
        assert_eq!(rowid, entry.id);
        assert_eq!(fts_content, "银行卡号 622202");
    }

    #[test]
    fn test_sqlite_search_by_pinyin_first_and_full() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.insert_text("银行卡", "银行卡", "yhk yxk", "yinhangka yinxingka").unwrap();
        storage.insert_text("身份证", "身份证", "sfz", "shenfenzheng").unwrap();
        storage.insert_text("微信公众号", "微信公众号", "wxgzh", "weixingongzhonghao").unwrap();

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
        storage.insert_text("2026年微信API接口规范", "2026年微信API接口规范", "2026nwxapijk", "2026nianweixinapijiekou").unwrap();
        storage.insert_text("2026年年度财务报告", "2026年年度财务报告", "2026nndcwbg", "2026niannianducaiwubaogao").unwrap();
        storage.insert_text("API设计指导原则", "API设计指导原则", "apisj zdyz", "apishejizhidaoyuanze").unwrap();

        // 多词空格分割 AND 匹配: "2026 api" 仅能匹配同时满足的条目
        let res = storage.search_entries("2026 api", 10).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].content, "2026年微信API接口规范");
    }

    #[test]
    fn test_sqlite_insert_image_and_ocr() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let blob_name = "test_blob_hash_123.png";
        let entry = storage
            .insert_image(blob_name, "[图片]", "tp", "tupian")
            .unwrap();

        assert_eq!(entry.entry_type, "image");
        assert_eq!(entry.content, blob_name);

        // 验证可查询到被引用的图片 blob
        let all_blobs = storage.get_all_image_contents().unwrap();
        assert_eq!(all_blobs, vec![blob_name.to_string()]);

        // 更新 OCR 提取文本
        storage
            .update_entry_ocr(entry.id, "发票金额：￥500.00", "fpje", "fapiaojine")
            .unwrap();

        // 验证全文索引已更新，支持基于 OCR 内容检索到该图片
        let search_res = storage.search_entries("发票", 10).unwrap();
        assert_eq!(search_res.len(), 1);
        assert_eq!(search_res[0].id, entry.id);
        assert_eq!(search_res[0].entry_type, "image");
    }

    #[test]
    fn test_sqlite_snippets_crud_and_isolation_from_lru() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        // 初始已有 3 个预置种子模板
        let initial = storage.get_all_snippets().unwrap();
        assert_eq!(initial.len(), 3);

        // 新建自定义短语 (带前缀斜杠输入测试)
        let created = storage
            .save_snippet(None, "请假申请", "主管您好，因个人事务申请请假一日。", "/leave")
            .unwrap();
        assert!(created.id > 0);
        assert_eq!(created.title, "请假申请");
        assert_eq!(created.shortcut, "leave", "快捷缩写的前缀斜杠必须被清洗剔除");

        // 编辑更新短语
        let updated = storage
            .save_snippet(Some(created.id), "病假申请", "因身体不适申请病假一日。", "sick")
            .unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.title, "病假申请");
        assert_eq!(updated.shortcut, "sick");

        // 验证剪贴板条目 LRU 清理彻底不影响短语库
        storage.insert_text("item 1", "item 1", "", "").unwrap();
        storage.prune_lru(0).unwrap();
        let snippets_after_lru = storage.get_all_snippets().unwrap();
        assert_eq!(snippets_after_lru.len(), 4, "短语库必须独立持久化且不受 LRU 淘汰影响");

        // 删除短语
        let deleted = storage.delete_snippet(created.id).unwrap();
        assert!(deleted);
        let by_id = storage.get_snippet_by_id(created.id).unwrap();
        assert!(by_id.is_none());

        // 验证即使全部删除后再次调用 init_tables 也绝不复活种子模板
        let all_current = storage.get_all_snippets().unwrap();
        for item in all_current {
            storage.delete_snippet(item.id).unwrap();
        }
        assert_eq!(storage.get_all_snippets().unwrap().len(), 0);
        storage.init_tables().unwrap();
        assert_eq!(storage.get_all_snippets().unwrap().len(), 0, "用户主动清空短语库后种子模板绝不复活");
    }

    #[test]
    fn test_sqlite_snippets_search() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        // 1. 测试 / 前缀检索快捷命令
        let res_meet = storage.search_snippets("/meet").unwrap();
        assert_eq!(res_meet.len(), 1);
        assert_eq!(res_meet[0].shortcut, "meet");
        assert_eq!(res_meet[0].title, "今日站会汇报");

        // 2. 测试直接按名称检索
        let res_time = storage.search_snippets("时间戳").unwrap();
        assert_eq!(res_time.len(), 1);
        assert_eq!(res_time[0].shortcut, "time");

        // 3. 测试中文拼音首字母简拼检索 (AC-3 拼音匹配短语)
        let res_pinyin = storage.search_snippets("jrzh").unwrap();
        assert_eq!(res_pinyin.len(), 1);
        assert_eq!(res_pinyin[0].title, "今日站会汇报");
    }
}


