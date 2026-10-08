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

/// 常用短语模板实体 (Snippet)
///
/// 遵循 CONTEXT.md 领域模型定义，包含标题、内容模板、快捷缩写及时间戳。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// 唯一主键自增 ID
    pub id: i64,
    /// 短语标题说明 (如 "今日站会汇报")
    pub title: String,
    /// 模板原始内容 (支持包含 {current_date}、{time} 等占位符)
    pub content: String,
    /// 快捷命令缩写 (如 "meet"，主检索框输入 /meet 时直接匹配)
    pub shortcut: String,
    /// 创建毫秒时间戳
    pub created_at: i64,
    /// 最近更新毫秒时间戳
    pub updated_at: i64,
}

/// 存储驱动统一契约 (Storage Trait)
pub trait Storage: Send + Sync {
    /// 插入纯文本条目及检索索引
    ///
    /// @param text 完整纯文本内容
    /// @param fts_content 全文检索截断索引内容 (超大文本截取前 200KB)
    /// @param pinyin_first 拼音首字母简拼索引
    /// @param pinyin_full 拼音全拼索引
    /// @return 成功返回持久化后的剪贴板条目对象
    fn insert_text(
        &self,
        text: &str,
        fts_content: &str,
        pinyin_first: &str,
        pinyin_full: &str,
    ) -> Result<ClipboardEntry, StorageError>;

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

    /// 基于关键词与拼音进行模糊检索，支持空格分割多词 AND 匹配
    ///
    /// @param query 检索关键词（支持中文、拼音简拼/全拼、英文多词）
    /// @param limit 最大返回条数
    /// @return 匹配的历史条目列表
    fn search_entries(&self, query: &str, limit: usize) -> Result<Vec<ClipboardEntry>, StorageError>;

    /// 切换指定条目的置顶固定状态 (Pin / Unpin)
    ///
    /// @param id 条目主键 ID
    /// @return 切换后的置顶状态 (true 为已置顶，false 为取消置顶)
    fn toggle_pin(&self, id: i64) -> Result<bool, StorageError>;

    /// 精确根据纯文本内容查找历史已有条目
    ///
    /// @param text 待查找的纯文本
    /// @return 存在返回 Some(ClipboardEntry)，不存在返回 Ok(None)
    fn find_by_content(&self, text: &str) -> Result<Option<ClipboardEntry>, StorageError>;

    /// 刷新已有条目的时间戳并重新置顶到最前 (Bump-to-Top)
    ///
    /// @param id 条目主键 ID
    /// @return 更新后的条目实体
    fn bump_to_top(&self, id: i64) -> Result<ClipboardEntry, StorageError>;

    /// 插入图片多媒体条目及检索索引
    ///
    /// @param blob_name 对应磁盘中的 Blob 文件名
    /// @param ocr_text 初步 OCR 文本或占位描述
    /// @param pinyin_first 拼音首字母简拼索引
    /// @param pinyin_full 拼音全拼索引
    /// @return 成功返回持久化后的图片条目对象
    fn insert_image(
        &self,
        blob_name: &str,
        ocr_text: &str,
        pinyin_first: &str,
        pinyin_full: &str,
    ) -> Result<ClipboardEntry, StorageError>;

    /// 更新指定条目的 OCR 提取文本与全文索引
    ///
    /// @param id 条目主键 ID
    /// @param ocr_text 识别得到的文本内容
    /// @param pinyin_first 拼音首字母简拼索引
    /// @param pinyin_full 拼音全拼索引
    fn update_entry_ocr(
        &self,
        id: i64,
        ocr_text: &str,
        pinyin_first: &str,
        pinyin_full: &str,
    ) -> Result<(), StorageError>;

    /// 获取数据库中所有被引用的图片 Blob 文件名列表 (用于 Blob GC 垃圾回收)
    ///
    /// @return 数据库当前引用的图片 blob 名称集合
    fn get_all_image_contents(&self) -> Result<Vec<String>, StorageError>;

    /// 执行 LRU 容量淘汰清理，永久豁免置顶条目
    ///
    /// @param max_capacity 非置顶条目最大保留容量 (如 1000)
    /// @return 实际淘汰清理的记录条数
    fn prune_lru(&self, max_capacity: usize) -> Result<usize, StorageError>;

    /// 保存常用短语模板 (若 id 为 Some 则更新，否则新建)
    ///
    /// @param id 指定 ID 则更新，None 则新建
    /// @param title 短语标题
    /// @param content 短语模板内容
    /// @param shortcut 快捷缩写
    /// @return 成功返回持久化后的短语实体
    fn save_snippet(
        &self,
        id: Option<i64>,
        title: &str,
        content: &str,
        shortcut: &str,
    ) -> Result<Snippet, StorageError>;

    /// 删除指定常用短语模板
    ///
    /// @param id 短语主键 ID
    /// @return 删除成功返回 true，不存在返回 false
    fn delete_snippet(&self, id: i64) -> Result<bool, StorageError>;

    /// 获取全部常用短语模板列表 (按更新时间倒序)
    ///
    /// @return 短语列表，无数据返回空集合
    fn get_all_snippets(&self) -> Result<Vec<Snippet>, StorageError>;

    /// 根据唯一 ID 查询常用短语
    ///
    /// @param id 短语主键 ID
    /// @return 存在返回 Some(Snippet)，不存在返回 Ok(None)
    fn get_snippet_by_id(&self, id: i64) -> Result<Option<Snippet>, StorageError>;

    /// 检索常用短语 (支持按前缀快捷缩写或关键词匹配)
    ///
    /// @param query 检索词 (如 "/meet" 或 "汇报")
    /// @return 匹配的短语列表
    fn search_snippets(&self, query: &str) -> Result<Vec<Snippet>, StorageError>;

    /// 读取持久化应用配置元数据
    ///
    /// @param key 配置键名
    /// @return 对应值，不存在返回 Ok(None)
    fn get_metadata(&self, key: &str) -> Result<Option<String>, StorageError>;

    /// 保存持久化应用配置元数据
    ///
    /// @param key 配置键名
    /// @param value 配置键值
    fn set_metadata(&self, key: &str, value: &str) -> Result<(), StorageError>;

    /// 获取数据库存储文件的实际文件路径（内存存储返回 None）
    fn db_path(&self) -> Option<&std::path::Path> {
        None
    }

    /// 获取数据库中的历史条目总数与常用短语总数
    fn get_total_counts(&self) -> Result<(usize, usize), StorageError>;

    /// 删除单条剪贴板历史记录
    ///
    /// @param id 条目主键 ID
    /// @return 成功删除返回 true，条目不存在返回 false
    fn delete_entry(&self, id: i64) -> Result<bool, StorageError>;

    /// 批量删除剪贴板历史记录
    ///
    /// @param ids 待删除的条目主键 ID 列表
    /// @return 实际删除的条目数
    fn delete_entries(&self, ids: &[i64]) -> Result<usize, StorageError>;
}

