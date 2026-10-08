//! 内存测试桩平台驱动 (MockPlatformDriver)，用于无头单元测试毫秒级确定性验证。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::pal::{PalError, PlatformDriver};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 内存测试桩平台驱动
pub struct MockPlatformDriver {
    clipboard_text: Mutex<Option<String>>,
    clipboard_image: Mutex<Option<Vec<u8>>>,
    last_written_image: Mutex<Option<Vec<u8>>>,
    ocr_result: Mutex<Option<String>>,
    paste_count: AtomicUsize,
    monitor_callback: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    is_ignored: AtomicBool,
    source_process: Mutex<Option<String>>,
}

impl MockPlatformDriver {
    /// 创建全新的 Mock 驱动实例
    pub fn new() -> Self {
        Self {
            clipboard_text: Mutex::new(None),
            clipboard_image: Mutex::new(None),
            last_written_image: Mutex::new(None),
            ocr_result: Mutex::new(None),
            paste_count: AtomicUsize::new(0),
            monitor_callback: Mutex::new(None),
            is_ignored: AtomicBool::new(false),
            source_process: Mutex::new(None),
        }
    }

    /// 获取模拟粘贴调用计数
    pub fn paste_count(&self) -> usize {
        self.paste_count.load(Ordering::SeqCst)
    }

    /// 模拟设置当前剪贴板隐私排除标记
    pub fn simulate_privacy_flag(&self, ignored: bool) {
        self.is_ignored.store(ignored, Ordering::SeqCst);
    }

    /// 模拟设置当前复制操作的来源进程名称
    pub fn simulate_source_process(&self, process: Option<&str>) {
        let mut guard = self.source_process.lock().unwrap();
        *guard = process.map(|s| s.to_string());
    }

    /// 模拟设置 OCR 识别输出
    pub fn simulate_ocr_result(&self, text: Option<&str>) {
        let mut guard = self.ocr_result.lock().unwrap();
        *guard = text.map(|s| s.to_string());
    }

    /// 获取最后一次写入剪贴板的图片数据
    pub fn last_written_image(&self) -> Option<Vec<u8>> {
        self.last_written_image.lock().unwrap().clone()
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

    /// 模拟设置剪贴板中的图片并触发监听事件
    pub fn simulate_clipboard_image_change(&self, new_img: Option<Vec<u8>>) {
        {
            let mut img_guard = self.clipboard_image.lock().unwrap();
            *img_guard = new_img;
        }
        let cb_opt = {
            let guard = self.monitor_callback.lock().unwrap();
            guard.clone()
        };
        if let Some(cb) = cb_opt {
            cb();
        }
    }

    /// 模拟测试根据锚点计算浮窗吸附坐标 (基于纯算法 Flip-fit)
    pub fn calculate_popover_position(
        &self,
        anchor: crate::pal::anchor::AnchorPoint,
        width: i32,
        height: i32,
        work_area: crate::pal::anchor::ScreenRect,
    ) -> (i32, i32) {
        crate::pal::anchor::calculate_flip_fit_position(
            anchor,
            width,
            height,
            work_area,
            crate::pal::anchor::DEFAULT_ANCHOR_MARGIN,
        )
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

    fn is_clipboard_ignored(&self) -> Result<bool, PalError> {
        Ok(self.is_ignored.load(Ordering::SeqCst))
    }

    fn get_clipboard_source_process(&self) -> Result<Option<String>, PalError> {
        let guard = self.source_process.lock().unwrap();
        Ok(guard.clone())
    }

    fn read_image(&self) -> Result<Option<Vec<u8>>, PalError> {
        let guard = self.clipboard_image.lock().unwrap();
        Ok(guard.clone())
    }

    fn write_image(&self, data: &[u8]) -> Result<(), PalError> {
        let mut guard = self.last_written_image.lock().unwrap();
        *guard = Some(data.to_vec());
        Ok(())
    }

    fn ocr_image(&self, _data: &[u8]) -> Result<String, PalError> {
        let guard = self.ocr_result.lock().unwrap();
        Ok(guard.clone().unwrap_or_default())
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
    fn test_mock_platform_driver_privacy_flags() {
        let driver = MockPlatformDriver::new();
        assert!(!driver.is_clipboard_ignored().unwrap());

        driver.simulate_privacy_flag(true);
        assert!(driver.is_clipboard_ignored().unwrap());

        driver.simulate_source_process(Some("1password.exe"));
        assert_eq!(
            driver.get_clipboard_source_process().unwrap(),
            Some("1password.exe".to_string())
        );
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

    #[test]
    fn test_mock_platform_driver_image_and_ocr() {
        let driver = Arc::new(MockPlatformDriver::new());
        assert_eq!(driver.read_image().unwrap(), None);

        let img_bytes = vec![0x42, 0x4D, 0x01, 0x02];
        driver.simulate_clipboard_image_change(Some(img_bytes.clone()));
        assert_eq!(driver.read_image().unwrap(), Some(img_bytes.clone()));

        driver.write_image(&img_bytes).unwrap();
        assert_eq!(driver.last_written_image(), Some(img_bytes));

        driver.simulate_ocr_result(Some("识别文字 2026"));
        let ocr = driver.ocr_image(b"dummy").unwrap();
        assert_eq!(ocr, "识别文字 2026");
    }

    #[test]
    fn test_mock_platform_driver_popover_anchor() {
        let driver = MockPlatformDriver::new();
        let work_area = crate::pal::anchor::ScreenRect::new(0, 0, 1920, 1080);
        let anchor = crate::pal::anchor::AnchorPoint::new(100, 200);
        let (x, y) = driver.calculate_popover_position(
            anchor,
            crate::pal::anchor::COMPACT_POPOVER_WIDTH,
            crate::pal::anchor::COMPACT_POPOVER_HEIGHT,
            work_area,
        );
        assert_eq!(x, 100);
        assert_eq!(y, 208);
    }
}
