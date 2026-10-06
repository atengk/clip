//! 内存测试桩平台驱动 (MockPlatformDriver)，用于无头单元测试毫秒级确定性验证。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::pal::{PalError, PlatformDriver};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 内存测试桩平台驱动
pub struct MockPlatformDriver {
    clipboard_text: Mutex<Option<String>>,
    paste_count: AtomicUsize,
    monitor_callback: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl MockPlatformDriver {
    /// 创建全新的 Mock 驱动实例
    pub fn new() -> Self {
        Self {
            clipboard_text: Mutex::new(None),
            paste_count: AtomicUsize::new(0),
            monitor_callback: Mutex::new(None),
        }
    }

    /// 获取模拟粘贴调用计数
    pub fn paste_count(&self) -> usize {
        self.paste_count.load(Ordering::SeqCst)
    }

    /// 人工触发剪贴板变更事件，通知已注册的回调函数
    pub fn simulate_clipboard_change(&self, new_text: Option<String>) {
        {
            let mut text_guard = self.clipboard_text.lock().unwrap();
            *text_guard = new_text;
        }
        let cb_opt = {
            let guard = self.monitor_callback.lock().unwrap();
            guard.clone()
        };
        if let Some(cb) = cb_opt {
            cb();
        }
    }
}

impl Default for MockPlatformDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformDriver for MockPlatformDriver {
    fn read_text(&self) -> Result<Option<String>, PalError> {
        let guard = self.clipboard_text.lock().unwrap();
        Ok(guard.clone())
    }

    fn write_text(&self, text: &str) -> Result<(), PalError> {
        let mut guard = self.clipboard_text.lock().unwrap();
        *guard = Some(text.to_string());
        Ok(())
    }

    fn send_paste(&self) -> Result<(), PalError> {
        self.paste_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn start_monitor(&self, callback: Arc<dyn Fn() + Send + Sync>) -> Result<(), PalError> {
        let mut guard = self.monitor_callback.lock().unwrap();
        *guard = Some(callback);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_platform_driver_read_write_and_paste() {
        let driver = MockPlatformDriver::new();
        assert_eq!(driver.read_text().unwrap(), None);

        driver.write_text("Hello Clip").unwrap();
        assert_eq!(driver.read_text().unwrap(), Some("Hello Clip".to_string()));

        assert_eq!(driver.paste_count(), 0);
        driver.send_paste().unwrap();
        assert_eq!(driver.paste_count(), 1);
    }

    #[test]
    fn test_mock_platform_driver_monitor_callback() {
        let driver = Arc::new(MockPlatformDriver::new());
        let triggered = Arc::new(AtomicUsize::new(0));
        let triggered_clone = triggered.clone();

        driver
            .start_monitor(Arc::new(move || {
                triggered_clone.fetch_add(1, Ordering::SeqCst);
            }))
            .unwrap();

        assert_eq!(triggered.load(Ordering::SeqCst), 0);
        driver.simulate_clipboard_change(Some("Test Content".to_string()));
        assert_eq!(triggered.load(Ordering::SeqCst), 1);
        assert_eq!(
            driver.read_text().unwrap(),
            Some("Test Content".to_string())
        );
    }
}
