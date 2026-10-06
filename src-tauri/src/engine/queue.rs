//! 队列连贴状态机与管理器 (Paste Queue Manager)。
//!
//! 实现批量填表与数据迁移工作流中的先进先出 (FIFO) 连续收集与逐次出队回填。
//!
//! @author Ateng
//! @since 2026-10-06

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// 队列连贴单项数据结构
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueItem {
    /// 条目唯一主键 ID
    pub id: i64,
    /// 载荷内容 (纯文本或图片 Blob 名称)
    pub content: String,
    /// 条目类型 ("text" 或 "image")
    pub entry_type: String,
}

/// 队列连贴当前全局状态视图
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueStatus {
    /// 是否正处于收集或出队连贴激活模式
    pub is_active: bool,
    /// 当前队列中等待出队的剩余条目数
    pub count: usize,
    /// 队列中剩余条目列表快照
    pub items: Vec<QueueItem>,
}

/// 队列连贴状态机管理器
pub struct PasteQueueManager {
    /// 连贴模式激活标志
    is_active: AtomicBool,
    /// FIFO 双端队列
    queue: Mutex<VecDeque<QueueItem>>,
}

impl PasteQueueManager {
    /// 创建全新的队列连贴管理器实例
    pub fn new() -> Self {
        Self {
            is_active: AtomicBool::new(false),
            queue: Mutex::new(VecDeque::new()),
        }
    }

    /// 开启连贴收集模式，初始化空队列
    pub fn start(&self) {
        self.is_active.store(true, Ordering::SeqCst);
        let mut q = self.queue.lock().unwrap();
        q.clear();
    }

    /// 停止并退出连贴模式，清空待出队条目
    pub fn stop(&self) {
        self.is_active.store(false, Ordering::SeqCst);
        let mut q = self.queue.lock().unwrap();
        q.clear();
    }

    /// 切换连贴收集模式激活状态
    ///
    /// @return 切换后的最新激活状态 (true 为开启，false 为关闭)
    pub fn toggle(&self) -> bool {
        if self.is_active() {
            self.stop();
            false
        } else {
            self.start();
            true
        }
    }

    /// 查询当前连贴模式是否激活
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
    }

    /// 压入一条新复制的内容到队列尾部 (FIFO)
    ///
    /// 仅在连贴模式激活时入队；若未激活则静默丢弃。
    ///
    /// @param item 待入队项
    /// @return 若成功入队返回 true，若未处于激活状态返回 false
    pub fn push(&self, item: QueueItem) -> bool {
        if !self.is_active() {
            return false;
        }
        let mut q = self.queue.lock().unwrap();
        q.push_back(item);
        true
    }

    /// 从队头弹出一项待粘贴内容 (FIFO)
    ///
    /// 若出队后队列变为空，自动将激活状态重置为 false，完成生命周期闭环。
    ///
    /// @return 弹出的项，若队列已空返回 None
    pub fn pop(&self) -> Option<QueueItem> {
        let mut q = self.queue.lock().unwrap();
        let item = q.pop_front();
        if q.is_empty() {
            self.is_active.store(false, Ordering::SeqCst);
        }
        item
    }

    /// 获取当前队列中剩余待粘贴项总数
    pub fn len(&self) -> usize {
        let q = self.queue.lock().unwrap();
        q.len()
    }

    /// 判断当前队列是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 获取当前队列的完整状态快照
    pub fn get_status(&self) -> QueueStatus {
        let is_active = self.is_active();
        let q = self.queue.lock().unwrap();
        QueueStatus {
            is_active,
            count: q.len(),
            items: q.iter().cloned().collect(),
        }
    }
}

impl Default for PasteQueueManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paste_queue_lifecycle_fifo() {
        let manager = PasteQueueManager::new();
        assert!(!manager.is_active());
        assert_eq!(manager.len(), 0);

        // 未开启时 push 应该失败
        let pushed = manager.push(QueueItem {
            id: 1,
            content: "ignored".into(),
            entry_type: "text".into(),
        });
        assert!(!pushed);
        assert_eq!(manager.len(), 0);

        // 开启连贴模式
        manager.start();
        assert!(manager.is_active());

        // 依次压入三项 A, B, C
        manager.push(QueueItem {
            id: 1,
            content: "Item A".into(),
            entry_type: "text".into(),
        });
        manager.push(QueueItem {
            id: 2,
            content: "Item B".into(),
            entry_type: "text".into(),
        });
        manager.push(QueueItem {
            id: 3,
            content: "Item C".into(),
            entry_type: "text".into(),
        });

        assert_eq!(manager.len(), 3);
        let status = manager.get_status();
        assert!(status.is_active);
        assert_eq!(status.count, 3);
        assert_eq!(status.items[0].content, "Item A");

        // 验证先进先出 FIFO 出队顺序
        let pop1 = manager.pop().unwrap();
        assert_eq!(pop1.content, "Item A");
        assert_eq!(manager.len(), 2);
        assert!(manager.is_active(), "还有剩余项时应保持激活态");

        let pop2 = manager.pop().unwrap();
        assert_eq!(pop2.content, "Item B");
        assert_eq!(manager.len(), 1);
        assert!(manager.is_active());

        let pop3 = manager.pop().unwrap();
        assert_eq!(pop3.content, "Item C");
        assert_eq!(manager.len(), 0);
        assert!(!manager.is_active(), "队列清空后必须自动退出连贴模式 (AC-3)");

        // 再次出队返回 None
        assert!(manager.pop().is_none());
    }

    #[test]
    fn test_paste_queue_toggle_and_stop() {
        let manager = PasteQueueManager::new();

        // toggle 开启
        let active = manager.toggle();
        assert!(active);
        assert!(manager.is_active());

        manager.push(QueueItem {
            id: 1,
            content: "Data".into(),
            entry_type: "text".into(),
        });
        assert_eq!(manager.len(), 1);

        // toggle 关闭 (应清空队列并退出)
        let active2 = manager.toggle();
        assert!(!active2);
        assert!(!manager.is_active());
        assert_eq!(manager.len(), 0);
    }
}
