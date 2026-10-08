//! 窗口吸附定位与四向碰撞翻转算法 (WindowAnchor & Flip-fit Anchor)。
//!
//! @author Ateng
//! @since 2026-10-08

/// 屏幕矩形区域定义 (物理像素坐标)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// 锚点坐标 (文本输入光标插入符或鼠标指针)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorPoint {
    pub x: i32,
    pub y: i32,
}

impl AnchorPoint {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// 完整管理面板默认宽度与高度
pub const FULL_WINDOW_WIDTH: i32 = 660;
pub const FULL_WINDOW_HEIGHT: i32 = 520;

/// 默认光标与浮窗边距 (像素)
pub const DEFAULT_ANCHOR_MARGIN: i32 = 8;

/// 基于锚点坐标和显示器工作区边界计算四向自适应贴靠坐标 (Flip-fit Anchor)
///
/// 算法规则：
/// 1. 默认尝试放置在光标右下方：`x = anchor.x`，`y = anchor.y + margin`；
/// 2. 横向碰撞检测：若右侧越界（`x + win_width > work_area.right`），则水平翻转至光标左侧（`x = anchor.x - win_width`）；
/// 3. 纵向碰撞检测：若下方越界（`y + win_height > work_area.bottom`），则垂直翻转至光标上方（`y = anchor.y - win_height - margin`）；
/// 4. 最终边界钳位（Clamp）：确保整体坐标 `[x, x + win_width]` 与 `[y, y + win_height]` 严格落入 `work_area` 内部。
///
/// @param anchor 目标锚点 (文本光标或鼠标指针)
/// @param win_width 窗口目标宽度
/// @param win_height 窗口目标高度
/// @param work_area 所在显示器的有效可用工作区 (已排除任务栏)
/// @param margin 光标与窗口的间隙像素
/// @return 最终物理像素放置坐标 (x, y)
pub fn calculate_flip_fit_position(
    anchor: AnchorPoint,
    win_width: i32,
    win_height: i32,
    work_area: ScreenRect,
    margin: i32,
) -> (i32, i32) {
    // 1. 默认尝试放置在光标右侧下方
    let mut x = anchor.x;
    let mut y = anchor.y + margin;

    // 2. 水平碰撞检测与向左翻转
    if x + win_width > work_area.right {
        x = anchor.x - win_width;
    }

    // 3. 垂直碰撞检测与向上翻转
    if y + win_height > work_area.bottom {
        y = anchor.y - win_height - margin;
    }

    // 4. 工作区边界强制钳位 (Clamp 防溢出)
    let max_x = work_area.right - win_width;
    if x > max_x {
        x = max_x;
    }
    if x < work_area.left {
        x = work_area.left;
    }

    let max_y = work_area.bottom - win_height;
    if y > max_y {
        y = max_y;
    }
    if y < work_area.top {
        y = work_area.top;
    }

    (x, y)
}

/// 将系统虚拟键码（VK_CODE）及修饰键状态映射为免激活面板导航控制行为
///
/// 严格防御系统级快捷键（例如 Alt+Tab 窗口切换、Ctrl+C/V 等），在此类场景下返回 None，绝不拦截阻断。
///
/// @param vk_code 虚拟键码
/// @param is_alt_down Alt 键是否处于按下态
/// @param is_ctrl_down Ctrl 键是否处于按下态
/// @return 对应的导航动作标识字符串，若无需接管则返回 None
pub fn map_vk_to_popover_action(
    vk_code: u16,
    is_alt_down: bool,
    is_ctrl_down: bool,
) -> Option<&'static str> {
    // 若携带 Alt 或 Ctrl 修饰键，一律不予拦截，保证 Alt+Tab、Ctrl+W 等系统快捷键畅通无阻
    if is_alt_down || is_ctrl_down {
        return None;
    }

    match vk_code {
        0x26 => Some("up"),      // VK_UP
        0x28 => Some("down"),    // VK_DOWN
        0x0D => Some("enter"),   // VK_RETURN
        0x1B => Some("escape"),  // VK_ESCAPE
        0x09 => Some("tab"),     // VK_TAB
        // 0x31..=0x39: 主键盘数字键 1~9
        0x31..=0x39 => {
            const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
            Some(DIGITS[(vk_code - 0x31) as usize])
        }
        // 0x61..=0x69: 小键盘数字键 1~9
        0x61..=0x69 => {
            const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
            Some(DIGITS[(vk_code - 0x61) as usize])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_vk_navigation_keys() {
        // 纯净导航按键
        assert_eq!(map_vk_to_popover_action(0x26, false, false), Some("up"));
        assert_eq!(map_vk_to_popover_action(0x28, false, false), Some("down"));
        assert_eq!(map_vk_to_popover_action(0x0D, false, false), Some("enter"));
        assert_eq!(map_vk_to_popover_action(0x1B, false, false), Some("escape"));
        assert_eq!(map_vk_to_popover_action(0x09, false, false), Some("tab"));

        // 包含 Alt 修饰键的 Tab (如 Alt+Tab) 绝不拦截！
        assert_eq!(map_vk_to_popover_action(0x09, true, false), None);
        // 包含 Ctrl 修饰键的 Tab 绝不拦截！
        assert_eq!(map_vk_to_popover_action(0x09, false, true), None);
    }

    #[test]
    fn test_map_vk_fast_paste_digits() {
        // 主键盘数字键 1~9
        for (i, code) in (0x31..=0x39).enumerate() {
            let expected = format!("{}", i + 1);
            assert_eq!(
                map_vk_to_popover_action(code, false, false),
                Some(expected.as_str())
            );
        }

        // 小键盘数字键 1~9
        for (i, code) in (0x61..=0x69).enumerate() {
            let expected = format!("{}", i + 1);
            assert_eq!(
                map_vk_to_popover_action(code, false, false),
                Some(expected.as_str())
            );
        }

        // 带有修饰键的数字组合（如 Ctrl+1）不予拦截
        assert_eq!(map_vk_to_popover_action(0x31, false, true), None);
        assert_eq!(map_vk_to_popover_action(0x31, true, false), None);

        // 未知或未关心的按键（如字母 A: 0x41, 空格: 0x20）不予拦截
        assert_eq!(map_vk_to_popover_action(0x41, false, false), None);
        assert_eq!(map_vk_to_popover_action(0x20, false, false), None);
    }

    #[test]
    fn test_normal_placement_within_bounds() {
        let work_area = ScreenRect::new(0, 0, 1920, 1040);
        let anchor = AnchorPoint::new(200, 200);
        let (x, y) = calculate_flip_fit_position(
            anchor,
            FULL_WINDOW_WIDTH,
            FULL_WINDOW_HEIGHT,
            work_area,
            DEFAULT_ANCHOR_MARGIN,
        );

        // 默认放置在右下方：x = 200, y = 200 + 8 = 208
        assert_eq!(x, 200);
        assert_eq!(y, 208);
    }

    #[test]
    fn test_right_boundary_flip_left() {
        let work_area = ScreenRect::new(0, 0, 1920, 1040);
        // 靠近右边缘
        let anchor = AnchorPoint::new(1800, 200);
        let (x, y) = calculate_flip_fit_position(
            anchor,
            FULL_WINDOW_WIDTH,
            FULL_WINDOW_HEIGHT,
            work_area,
            DEFAULT_ANCHOR_MARGIN,
        );

        // 1800 + 660 = 2460 > 1920，向左翻转：x = 1800 - 660 = 1140
        assert_eq!(x, 1140);
        assert_eq!(y, 208);
        assert!(x + FULL_WINDOW_WIDTH <= work_area.right);
    }

    #[test]
    fn test_bottom_boundary_flip_up() {
        let work_area = ScreenRect::new(0, 0, 1920, 1040);
        // 靠近底边任务栏
        let anchor = AnchorPoint::new(200, 900);
        let (x, y) = calculate_flip_fit_position(
            anchor,
            FULL_WINDOW_WIDTH,
            FULL_WINDOW_HEIGHT,
            work_area,
            DEFAULT_ANCHOR_MARGIN,
        );

        // 900 + 520 + 8 = 1428 > 1040，向上翻转：y = 900 - 520 - 8 = 372
        assert_eq!(x, 200);
        assert_eq!(y, 372);
        assert!(y + FULL_WINDOW_HEIGHT <= work_area.bottom);
    }

    #[test]
    fn test_bottom_right_corner_double_flip() {
        let work_area = ScreenRect::new(0, 0, 1920, 1040);
        // 处于屏幕极右下角
        let anchor = AnchorPoint::new(1900, 1000);
        let (x, y) = calculate_flip_fit_position(
            anchor,
            FULL_WINDOW_WIDTH,
            FULL_WINDOW_HEIGHT,
            work_area,
            DEFAULT_ANCHOR_MARGIN,
        );

        // 双向翻转：向左且向上
        assert_eq!(x, 1900 - FULL_WINDOW_WIDTH); // 1240
        assert_eq!(y, 1000 - FULL_WINDOW_HEIGHT - DEFAULT_ANCHOR_MARGIN); // 472
        assert!(x >= work_area.left && x + FULL_WINDOW_WIDTH <= work_area.right);
        assert!(y >= work_area.top && y + FULL_WINDOW_HEIGHT <= work_area.bottom);
    }

    #[test]
    fn test_negative_coordinates_multi_monitor() {
        // 副屏在主屏左侧，坐标为负数
        let work_area = ScreenRect::new(-1920, 0, 0, 1080);
        let anchor = AnchorPoint::new(-200, 200);
        let (x, y) = calculate_flip_fit_position(
            anchor,
            FULL_WINDOW_WIDTH,
            FULL_WINDOW_HEIGHT,
            work_area,
            DEFAULT_ANCHOR_MARGIN,
        );

        // -200 + 660 = 460 > 0 (右侧越界)，向左翻转：-200 - 660 = -860
        assert_eq!(x, -860);
        assert_eq!(y, 208);
        assert!(x >= work_area.left && x + FULL_WINDOW_WIDTH <= work_area.right);
    }

    #[test]
    fn test_extreme_clamp_when_window_exceeds_monitor() {
        // 小屏幕分辨率比如 500x400，窗口尺寸 660x520
        let work_area = ScreenRect::new(0, 0, 500, 400);
        let anchor = AnchorPoint::new(100, 100);
        let (x, y) = calculate_flip_fit_position(
            anchor,
            FULL_WINDOW_WIDTH,
            FULL_WINDOW_HEIGHT,
            work_area,
            DEFAULT_ANCHOR_MARGIN,
        );

        // 极端情况下钳位至工作区左上角
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }
}
