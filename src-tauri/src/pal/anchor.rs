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

#[cfg(test)]
mod tests {
    use super::*;

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
