//! 隐身无痕模式状态机 (Incognito Mode Manager)。
//!
//! 支持会议演示或私密操作场景下临时旁路剪贴板事件监听，支持手动退出与定时自动恢复（如 15 分钟、1 小时）。
//!
//! @author Ateng
//! @since 2026-10-06

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 隐身模式当前全局状态视图
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncognitoStatus {
    /// 是否正处于隐身激活模式
    pub is_active: bool,
    /// 剩余有效秒数（若为定时隐身；若为手动退出则为 None）
    pub remaining_seconds: Option<u64>,
}

/// 隐身模式状态机管理器
pub struct IncognitoManager {
    is_active: AtomicBool,
    deadline: Mutex<Option<Instant>>,
}

impl IncognitoManager {
    /// 创建全新的隐身模式管理器实例
    pub fn new() -> Self {
        Self {
            is_active: AtomicBool::new(false),
            deadline: Mutex::new(None),
        }
    }

    /// 开启隐身模式
    ///
    /// @param duration_minutes 可选持续分钟数（None 表示手动退出，Some(m) 表示定时到期后自动恢复）
    pub fn enter(&self, duration_minutes: Option<u64>) {
        let deadline = duration_minutes.map(|m| Instant::now() + Duration::from_secs(m * 60));
        {
            let mut guard = self.deadline.lock().unwrap();
            *guard = deadline;
        }
        self.is_active.store(true, Ordering::SeqCst);
    }

    /// 退出隐身模式，恢复常规剪贴板监听
    pub fn leave(&self) {
        {
            let mut guard = self.deadline.lock().unwrap();
            *guard = None;
        }
        self.is_active.store(false, Ordering::SeqCst);
    }

    /// 切换隐身模式状态
    ///
    /// @param duration_minutes 开启时指定的持续分钟数
    /// @return 切换后的最新激活状态 (true 为开启，false 为关闭)
    pub fn toggle(&self, duration_minutes: Option<u64>) -> bool {
        if self.is_active() {
            self.leave();
            false
        } else {
            self.enter(duration_minutes);
            true
        }
    }

    /// 查询当前是否正处于隐身激活模式
    ///
    /// 若定时已到期，内部自动将状态复位为 false 闭环。
    pub fn is_active(&self) -> bool {
        if !self.is_active.load(Ordering::SeqCst) {
            return false;
        }

        let mut guard = self.deadline.lock().unwrap();
        if let Some(dl) = *guard {
            if Instant::now() >= dl {
                *guard = None;
                self.is_active.store(false, Ordering::SeqCst);
                return false;
            }
        }

        true
    }

    /// 获取隐身模式当前状态快照
    pub fn get_status(&self) -> IncognitoStatus {
        let active = self.is_active();
        let guard = self.deadline.lock().unwrap();
        let remaining_seconds = if active {
            guard.as_ref().and_then(|dl| {
                let now = Instant::now();
                if *dl > now {
                    Some((*dl - now).as_secs())
                } else {
                    None
                }
            })
        } else {
            None
        };

        IncognitoStatus {
            is_active: active,
            remaining_seconds,
        }
    }
}

impl Default for IncognitoManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incognito_manual_enter_and_leave() {
        let manager = IncognitoManager::new();
        assert!(!manager.is_active());
        assert!(!manager.get_status().is_active);

        // 手动开启隐身模式
        manager.enter(None);
        assert!(manager.is_active());
        let status = manager.get_status();
        assert!(status.is_active);
        assert_eq!(status.remaining_seconds, None);

        // 退出隐身模式
        manager.leave();
        assert!(!manager.is_active());
        assert!(!manager.get_status().is_active);
    }

    #[test]
    fn test_incognito_timed_and_expiry() {
        let manager = IncognitoManager::new();

        // 设定到期时间为 10ms 之后
        {
            let mut guard = manager.deadline.lock().unwrap();
            *guard = Some(Instant::now() + Duration::from_millis(20));
        }
        manager.is_active.store(true, Ordering::SeqCst);

        // 即刻检查应当处于激活态
        assert!(manager.is_active());

        // 等待定时到期
        std::thread::sleep(Duration::from_millis(35));

        // 到期后再次检查必须自动失效恢复
        assert!(!manager.is_active());
        assert!(!manager.get_status().is_active);
    }

    #[test]
    fn test_incognito_toggle() {
        let manager = IncognitoManager::new();
        assert!(!manager.is_active());

        // toggle 开启
        assert!(manager.toggle(Some(15)));
        assert!(manager.is_active());

        // toggle 关闭
        assert!(!manager.toggle(None));
        assert!(!manager.is_active());
    }
}
