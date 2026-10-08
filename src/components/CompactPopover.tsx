/**
 * 紧凑光标吸附小卡片视图组件 (Compact Popover)
 *
 * 用于 Alt+V 唤出时紧贴输入插入符或鼠标指针展示，
 * 支持 1~9 直贴标记、↑/↓ 键控导航自适应滚动及 Tab 平滑展开。
 *
 * @author Ateng
 * @since 2026-10-08
 */

import React, { useEffect, useRef } from "react";
import type { DisplayItem, DetectedKind } from "../App";

/**
 * 紧凑小卡片组件属性契约
 */
export interface CompactPopoverProps {
  /** 候选展示条目列表 */
  items: DisplayItem[];
  /** 当前选中的项索引 (0-based) */
  selectedIndex: number;
  /** 临时脱敏防窥揭示的条目 ID */
  temporaryRevealId?: number | null;
  /** 选择项变更回调 */
  onSelectIndex: (index: number) => void;
  /** 触发粘贴回填回调 */
  onPaste: (item: DisplayItem) => void;
  /** 平滑展开为完整面板回调 */
  onExpandToFull: () => void;
}

/**
 * 获取对应类型的语义化图标
 */
const getItemTypeIcon = (kind: DetectedKind): string => {
  switch (kind) {
    case "sensitive":
      return "🛡️";
    case "code":
      return "</>";
    case "link":
      return "🔗";
    case "image":
      return "🖼️";
    case "snippet":
      return "⚡";
    default:
      return "📄";
  }
};

export const CompactPopover: React.FC<CompactPopoverProps> = ({
  items,
  selectedIndex,
  temporaryRevealId,
  onSelectIndex,
  onPaste,
  onExpandToFull,
}) => {
  const selectedItemRef = useRef<HTMLDivElement | null>(null);

  // 当键盘导航切换选中项时，确保当前高亮条目始终在视口内可见
  useEffect(() => {
    if (selectedItemRef.current) {
      selectedItemRef.current.scrollIntoView({
        block: "nearest",
        behavior: "smooth",
      });
    }
  }, [selectedIndex]);

  return (
    <div className="compact-popover-container">
      {/* 紧凑头部 */}
      <div className="compact-popover-header">
        <div className="compact-brand">
          <span className="compact-logo-badge">📋</span>
          <span className="compact-title">Clip</span>
          <span className="compact-subtitle">剪贴板历史</span>
        </div>
        <button
          className="compact-expand-btn"
          onClick={onExpandToFull}
          title="展开为完整管理面板 (Tab)"
        >
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
          >
            <polyline points="15 3 21 3 21 9" />
            <polyline points="9 21 3 21 3 15" />
            <line x1="21" y1="3" x2="14" y2="10" />
            <line x1="3" y1="21" x2="10" y2="14" />
          </svg>
          <span className="compact-expand-tip">Tab 展开</span>
        </button>
      </div>

      {/* 紧凑候选条目列表 (支持完整滚动浏览) */}
      <div className="compact-popover-list">
        {items.length === 0 ? (
          <div className="compact-empty">暂无剪贴板历史</div>
        ) : (
          items.map((item, index) => {
            const isSelected = index === selectedIndex;
            const fastKey = index < 9 ? String(index + 1) : null;
            const typeIcon = getItemTypeIcon(item.kind);

            return (
              <div
                key={item.id}
                ref={isSelected ? selectedItemRef : undefined}
                className={`compact-item ${isSelected ? "selected" : ""}`}
                onClick={() => onPaste(item)}
                onMouseEnter={() => onSelectIndex(index)}
              >
                <div className="compact-key-badge">{fastKey ?? ""}</div>
                <div className="compact-icon-slot">{typeIcon}</div>
                <div className="compact-content-text">
                  {item.kind === "sensitive" && item.id !== temporaryRevealId
                    ? "•••••••• (机密凭据)"
                    : item.content}
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* 紧凑状态底栏 */}
      <div className="compact-popover-footer">
        <span className="compact-foot-tip">
          <b>↑↓</b> 移动
        </span>
        <span className="compact-foot-tip">
          <b>1-9</b> 直贴
        </span>
        <span className="compact-foot-tip">
          <b>Enter</b> 粘贴
        </span>
        <span className="compact-foot-tip">
          <b>Esc</b> 关闭
        </span>
      </div>
    </div>
  );
};
