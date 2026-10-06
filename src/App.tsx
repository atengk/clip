/**
 * 悬浮剪贴板历史面板，集成拼音模糊搜索框、置顶管理、超大文本熔断标注与失焦自隐。
 *
 * @author Ateng
 * @since 2026-10-06
 */

import React, { useEffect, useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { maskSensitiveContent } from "./utils/privacy";
import "./App.css";

/**
 * 剪贴板条目数据结构 (遵循 CONTEXT.md)
 */
interface ClipboardEntry {
  id: number;
  content: string;
  entry_type: string;
  created_at: number;
  is_pinned: boolean;
}

/** 2MB 字节/字符阈值，用于前端展示超大文本标签与长字符串 DOM 裁剪保护 */
const LARGE_TEXT_THRESHOLD = 2 * 1024 * 1024;

/**
 * 支持的格式清洗与转换动作类型枚举
 */
export type ActionKey =
  | "trim"
  | "plain_text"
  | "uppercase"
  | "lowercase"
  | "camel_case"
  | "snake_case"
  | "json_prettify"
  | "json_minify";

/**
 * 文本清洗与格式转换动作项定义
 */
interface ActionItem {
  key: ActionKey;
  label: string;
  description: string;
  icon: string;
  hotkey: string;
}

const TRANSFORM_ACTIONS: ActionItem[] = [
  { key: "trim", label: "去除多余空白与换行 (Trim)", description: "剔除首尾空白，折叠连续多行空白", icon: "✂️", hotkey: "1" },
  { key: "plain_text", label: "强制纯文本 (Plain Text)", description: "剔除所有控制字符，规范换行", icon: "📄", hotkey: "2" },
  { key: "uppercase", label: "转为全部大写 (UPPERCASE)", description: "英文字符全部转为大写", icon: "🔠", hotkey: "3" },
  { key: "lowercase", label: "转为全部小写 (lowercase)", description: "英文字符全部转为小写", icon: "🔡", hotkey: "4" },
  { key: "camel_case", label: "转为小驼峰 (camelCase)", description: "转换为小驼峰变量规范", icon: "🐫", hotkey: "5" },
  { key: "snake_case", label: "转为下划线 (snake_case)", description: "转换为蛇形下划线规范", icon: "🐍", hotkey: "6" },
  { key: "json_prettify", label: "JSON 语法美化 (Prettify)", description: "校验 JSON 并按 2 空格缩进排版", icon: "✨", hotkey: "7" },
  { key: "json_minify", label: "JSON 紧凑压缩 (Minify)", description: "去除所有空行与缩进压缩为单行", icon: "📦", hotkey: "8" },
];

export const App: React.FC = () => {
  const [query, setQuery] = useState<string>("");
  const [entries, setEntries] = useState<ClipboardEntry[]>([]);
  const [selectedIndex, setSelectedIndex] = useState<number>(0);
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const [actionPaletteOpen, setActionPaletteOpen] = useState<boolean>(false);
  const [actionSelectedIndex, setActionSelectedIndex] = useState<number>(0);
  const [transformError, setTransformError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  /**
   * 加载或检索历史记录
   */
  const fetchEntries = useCallback(async (searchQuery: string) => {
    try {
      const q = searchQuery.trim();
      let history: ClipboardEntry[];
      if (q.length === 0) {
        history = await invoke<ClipboardEntry[]>("get_history", { limit: 50 });
      } else {
        history = await invoke<ClipboardEntry[]>("search_history", { query: q, limit: 50 });
      }
      setEntries(history);
      setSelectedIndex(0);
    } catch (err) {
      console.error("加载/检索剪贴板历史失败:", err);
    }
  }, []);

  /**
   * 触发指定条目的极速回填
   *
   * @param id 条目唯一主键 ID
   */
  const handlePaste = useCallback(async (id: number) => {
    try {
      await invoke("paste_entry", { id });
    } catch (err) {
      console.error("回填剪贴板条目失败:", err);
    }
  }, []);

  /**
   * 触发指定条目的动作转换与极速回填
   *
   * @param id 条目唯一主键 ID
   * @param actionKey 转换动作标识
   */
  const handleTransformAndPaste = useCallback(async (id: number, actionKey: ActionKey) => {
    setTransformError(null);
    try {
      await invoke("transform_and_paste_entry", { id, action: actionKey });
      setActionPaletteOpen(false);
    } catch (err: unknown) {
      const msg = typeof err === "string" ? err : "执行格式转换失败";
      setTransformError(msg);
      setTimeout(() => setTransformError(null), 3000);
    }
  }, []);

  /**
   * 强制以纯文本格式回填当前条目 (Shift + Enter 专用)
   *
   * @param id 条目唯一主键 ID
   */
  const handlePastePlain = useCallback(async (id: number) => {
    try {
      await invoke("paste_plain_entry", { id });
    } catch (err) {
      console.error("纯文本回填失败:", err);
    }
  }, []);

  /**
   * 切换指定条目的置顶固定状态 (Pin / Unpin)
   *
   * @param id 条目唯一 ID
   */
  const handleTogglePin = useCallback(
    async (id: number, e?: React.MouseEvent) => {
      if (e) {
        e.stopPropagation();
      }
      try {
        await invoke("toggle_pin", { id });
        fetchEntries(query);
      } catch (err) {
        console.error("切换置顶状态失败:", err);
      }
    },
    [fetchEntries, query]
  );

  /**
   * 主动隐藏悬浮面板
   */
  const handleClose = useCallback(async () => {
    setActionPaletteOpen(false);
    try {
      await invoke("hide_window");
    } catch (err) {
      console.error("隐藏窗口失败:", err);
    }
  }, []);

  // 搜索框输入联动
  const handleQueryChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.value;
    setQuery(val);
    fetchEntries(val);
  };

  // 监听后端广播事件与窗口唤起
  useEffect(() => {
    fetchEntries("");

    // 1. 监听系统剪贴板更新事件
    const unlistenClipboard = listen<ClipboardEntry>("clipboard-changed", () => {
      fetchEntries(query);
    });

    // 2. 监听窗口唤起展示事件 (初始化焦点与清空历史)
    const unlistenPanelShown = listen("panel-shown", () => {
      setQuery("");
      fetchEntries("");
      setSelectedIndex(0);
      setActionPaletteOpen(false);
      setTimeout(() => {
        inputRef.current?.focus();
        inputRef.current?.select();
      }, 20);
    });

    // 3. 页面失焦无感自隐防御
    const handleBlur = () => {
      handleClose();
    };
    window.addEventListener("blur", handleBlur);

    return () => {
      unlistenClipboard.then((f) => f());
      unlistenPanelShown.then((f) => f());
      window.removeEventListener("blur", handleBlur);
    };
  }, [fetchEntries, query, handleClose]);

  // 全局键盘导航处理
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // 1. 输入法合成状态保护：正在输入中文拼音时，Enter 仅用于确认上屏，绝不触发粘贴
      if (e.isComposing) {
        return;
      }

      // 2. Action Palette 处于激活态时的键盘路由
      if (actionPaletteOpen) {
        if (e.key === "Escape" || (e.ctrlKey && (e.key === "k" || e.key === "K")) || e.key === "Tab") {
          e.preventDefault();
          setActionPaletteOpen(false);
          return;
        }

        if (e.key === "ArrowDown") {
          e.preventDefault();
          setActionSelectedIndex((prev) => (prev < TRANSFORM_ACTIONS.length - 1 ? prev + 1 : 0));
          return;
        }

        if (e.key === "ArrowUp") {
          e.preventDefault();
          setActionSelectedIndex((prev) => (prev > 0 ? prev - 1 : TRANSFORM_ACTIONS.length - 1));
          return;
        }

        if (e.key === "Enter") {
          e.preventDefault();
          const current = entries[selectedIndex];
          const action = TRANSFORM_ACTIONS[actionSelectedIndex];
          if (current && action) {
            handleTransformAndPaste(current.id, action.key);
          }
          return;
        }

        // 数字键快捷触发对应动作 (1~8)
        if (e.key >= "1" && e.key <= "8") {
          const actionIdx = parseInt(e.key, 10) - 1;
          const current = entries[selectedIndex];
          const action = TRANSFORM_ACTIONS[actionIdx];
          if (current && action) {
            e.preventDefault();
            handleTransformAndPaste(current.id, action.key);
            return;
          }
        }

        // 动作浮层开启时，彻底拦截其它所有按键输入，杜绝穿透修改背景搜索框
        e.preventDefault();
        e.stopPropagation();
        return;
      }

      // 3. 主列表状态下的按键调度
      // Esc: 瞬间无感自隐并释放焦点
      if (e.key === "Escape") {
        e.preventDefault();
        handleClose();
        return;
      }

      // Tab 或 Ctrl+K: 唤出 Action Palette 动作浮层
      if ((e.ctrlKey && (e.key === "k" || e.key === "K")) || e.key === "Tab") {
        e.preventDefault();
        if (entries.length > 0 && entries[selectedIndex]) {
          setActionSelectedIndex(0);
          setActionPaletteOpen(true);
        }
        return;
      }

      // Shift + Enter: 强制纯文本格式极速回填
      if (e.shiftKey && e.key === "Enter") {
        e.preventDefault();
        const current = entries[selectedIndex];
        if (current) {
          handlePastePlain(current.id);
        }
        return;
      }

      // Alt + P 快捷切换当前选中项置顶
      if (e.altKey && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        const current = entries[selectedIndex];
        if (current) {
          handleTogglePin(current.id);
        }
        return;
      }

      // 动态序号极速回填：Alt + 1~9 强制回填，或输入框无内容/非输入态敲数字键回填
      const isNumberKey = e.key >= "1" && e.key <= "9";
      const shouldFastPasteNumber =
        (e.altKey && isNumberKey) ||
        (isNumberKey && (document.activeElement !== inputRef.current || query.length === 0));

      if (shouldFastPasteNumber) {
        const num = parseInt(e.key, 10);
        const targetEntry = entries[num - 1];
        if (targetEntry) {
          e.preventDefault();
          handlePaste(targetEntry.id);
          return;
        }
      }

      // 上下方向键导航
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((prev) => (prev < entries.length - 1 ? prev + 1 : prev));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((prev) => (prev > 0 ? prev - 1 : 0));
      } else if (e.key === "Enter") {
        // 回车极速回填当前高亮条目
        e.preventDefault();
        const current = entries[selectedIndex];
        if (current) {
          handlePaste(current.id);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    entries,
    selectedIndex,
    query,
    actionPaletteOpen,
    actionSelectedIndex,
    fetchEntries,
    handleClose,
    handlePaste,
    handlePastePlain,
    handleTransformAndPaste,
    handleTogglePin,
  ]);

  return (
    <div className="panel-container">
      <header className="panel-header" data-tauri-drag-region>
        <div className="search-bar">
          <span className="search-icon">🔍</span>
          <input
            ref={inputRef}
            type="text"
            className="search-input"
            value={query}
            onChange={handleQueryChange}
            placeholder="搜索剪贴板（支持中文拼音简拼如 yhk、全拼及多词空格）..."
            autoFocus
          />
        </div>
        <div className="shortcut-hints">
          <span className="hint-tag"><kbd>Tab</kbd> / <kbd>Ctrl+K</kbd> 动作</span>
          <span className="hint-tag"><kbd>Shift+↵</kbd> 纯文本</span>
          <span className="hint-tag"><kbd>1~9</kbd> 回填</span>
          <span className="hint-tag"><kbd>Alt+P</kbd> 置顶</span>
          <span className="hint-tag"><kbd>Esc</kbd> 自隐</span>
        </div>
      </header>

      <div className="panel-list" ref={listRef}>
        {entries.length === 0 ? (
          <div className="empty-state">
            <p>{query ? "未找到匹配条目" : "暂无剪贴板历史记录"}</p>
            <span className="empty-sub">
              {query ? "尝试更换拼音首字母简拼或模糊关键词" : "复制任意文本后将自动捕获并在此显示"}
            </span>
          </div>
        ) : (
          entries.map((item, index) => {
            const isSelected = index === selectedIndex;
            const fastPasteIndex = index < 9 ? index + 1 : null;
            const isLargeText = item.content.length > LARGE_TEXT_THRESHOLD;
            const { displayText: maskedText, isSensitive } = maskSensitiveContent(item.content);
            const isHovered = index === hoveredIndex;
            const isRevealed = isSensitive && isHovered;
            const activeText = isSensitive && !isRevealed ? maskedText : item.content;
            // 对超大文本进行 DOM 渲染截断保护，避免前端视图卡死
            const displayText = isLargeText ? activeText.slice(0, 300) + "..." : activeText;

            return (
              <div
                key={item.id}
                className={`panel-item ${isSelected ? "selected" : ""} ${item.is_pinned ? "pinned" : ""}`}
                onClick={() => handlePaste(item.id)}
                onMouseEnter={() => {
                  setSelectedIndex(index);
                  setHoveredIndex(index);
                }}
                onMouseLeave={() => {
                  if (hoveredIndex === index) {
                    setHoveredIndex(null);
                  }
                }}
              >
                <div className="item-badge">
                  {item.is_pinned ? (
                    <span className="badge-pin" title="置顶条目">📌</span>
                  ) : fastPasteIndex ? (
                    <span className="badge-num">{fastPasteIndex}</span>
                  ) : (
                    <span className="badge-dot">•</span>
                  )}
                </div>
                <div className="item-content">
                  <div className="item-text-line">
                    {isLargeText && <span className="tag-large">[超大文本]</span>}
                    {isSensitive && (
                      isRevealed ? (
                        <span className="tag-revealed" title="鼠标悬停已临时显隐明文，回填仍输出真实原文">👁️ 临时显隐</span>
                      ) : (
                        <span className="tag-masked" title="敏感凭据已防窥脱敏，鼠标悬停可临时显隐明文">🔒 掩码保护</span>
                      )
                    )}
                    <span className="item-text">{displayText}</span>
                  </div>
                </div>
                <div className="item-meta">
                  <button
                    className="action-btn"
                    onClick={(e) => {
                      e.stopPropagation();
                      setSelectedIndex(index);
                      setActionSelectedIndex(0);
                      setActionPaletteOpen(true);
                    }}
                    title="动作面板 (Ctrl+K / Tab)"
                  >
                    ⚡
                  </button>
                  <button
                    className={`pin-btn ${item.is_pinned ? "active" : ""}`}
                    onClick={(e) => handleTogglePin(item.id, e)}
                    title={item.is_pinned ? "取消置顶" : "置顶条目 (Alt+P)"}
                  >
                    {item.is_pinned ? "📌" : "📍"}
                  </button>
                  <span className="item-len">
                    {isLargeText
                      ? `${(item.content.length / (1024 * 1024)).toFixed(1)} MB`
                      : `${item.content.length} 字符`}
                  </span>
                </div>
              </div>
            );
          })
        )}
      </div>

      {actionPaletteOpen && entries[selectedIndex] && (
        <div
          className="action-palette-overlay"
          onClick={() => setActionPaletteOpen(false)}
        >
          <div
            className="action-palette"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="palette-header">
              <div className="palette-title">
                <span>⚡ 动作面板 (Action Palette)</span>
              </div>
              {transformError ? (
                <div className="palette-error">⚠️ {transformError}</div>
              ) : (
                <div className="palette-sub">
                  目标条目: {entries[selectedIndex].content.slice(0, 48).replace(/\n/g, " ")}...
                </div>
              )}
            </div>
            <div className="palette-list">
              {TRANSFORM_ACTIONS.map((action, idx) => {
                const isActionSelected = idx === actionSelectedIndex;
                return (
                  <div
                    key={action.key}
                    className={`palette-item ${isActionSelected ? "selected" : ""}`}
                    onClick={() => handleTransformAndPaste(entries[selectedIndex].id, action.key)}
                    onMouseEnter={() => setActionSelectedIndex(idx)}
                  >
                    <span className="palette-icon">{action.icon}</span>
                    <div className="palette-info">
                      <span className="palette-label">{action.label}</span>
                      <span className="palette-desc">{action.description}</span>
                    </div>
                    <span className="palette-hotkey">
                      <kbd>{action.hotkey}</kbd>
                    </span>
                  </div>
                );
              })}
            </div>
            <div className="palette-footer">
              <span><kbd>↵</kbd> / <kbd>1~8</kbd> 执行并回填</span>
              <span><kbd>Esc</kbd> 取消返回</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default App;
