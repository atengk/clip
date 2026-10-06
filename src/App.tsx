/**
 * 极简悬浮剪贴板历史面板，支持前 9 项数字键快速回填与上下键导航。
 *
 * @author Ateng
 * @since 2026-10-06
 */

import React, { useEffect, useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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

export const App: React.FC = () => {
  const [entries, setEntries] = useState<ClipboardEntry[]>([]);
  const [selectedIndex, setSelectedIndex] = useState<number>(0);
  const listRef = useRef<HTMLDivElement>(null);

  /**
   * 从后端拉取最新历史记录
   */
  const loadHistory = useCallback(async () => {
    try {
      const history = await invoke<ClipboardEntry[]>("get_history", { limit: 50 });
      setEntries(history);
      setSelectedIndex(0);
    } catch (err) {
      console.error("加载剪贴板历史记录失败:", err);
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
   * 主动隐藏悬浮面板
   */
  const handleClose = useCallback(async () => {
    try {
      await invoke("hide_window");
    } catch (err) {
      console.error("隐藏窗口失败:", err);
    }
  }, []);

  // 监听后端事件与快捷键重置
  useEffect(() => {
    loadHistory();

    // 1. 监听系统剪贴板更新事件
    const unlistenClipboard = listen<ClipboardEntry>("clipboard-changed", () => {
      loadHistory();
    });

    // 2. 监听窗口唤起展示事件
    const unlistenPanelShown = listen("panel-shown", () => {
      loadHistory();
      setSelectedIndex(0);
    });

    return () => {
      unlistenClipboard.then((f) => f());
      unlistenPanelShown.then((f) => f());
    };
  }, [loadHistory]);

  // 全局键盘快捷键导航 (1~9 Fast-Paste / 上下键 / Enter / Esc)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        handleClose();
        return;
      }

      // 数字键 1~9 极速回填 (Fast-Paste)
      if (e.key >= "1" && e.key <= "9") {
        const num = parseInt(e.key, 10);
        const targetEntry = entries[num - 1];
        if (targetEntry) {
          e.preventDefault();
          handlePaste(targetEntry.id);
          return;
        }
      }

      // 方向键与回车操作
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((prev) => (prev < entries.length - 1 ? prev + 1 : prev));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((prev) => (prev > 0 ? prev - 1 : 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const current = entries[selectedIndex];
        if (current) {
          handlePaste(current.id);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [entries, selectedIndex, handleClose, handlePaste]);

  return (
    <div className="panel-container">
      <header className="panel-header" data-tauri-drag-region>
        <span className="brand-title">📋 剪贴板历史</span>
        <div className="shortcut-hints">
          <span className="hint-tag"><kbd>1~9</kbd> 极速粘贴</span>
          <span className="hint-tag"><kbd>↵</kbd> 回填</span>
          <span className="hint-tag"><kbd>Esc</kbd> 关闭</span>
        </div>
      </header>

      <div className="panel-list" ref={listRef}>
        {entries.length === 0 ? (
          <div className="empty-state">
            <p>暂无剪贴板历史记录</p>
            <span className="empty-sub">复制任意文本后将自动捕获并在此显示</span>
          </div>
        ) : (
          entries.map((item, index) => {
            const isSelected = index === selectedIndex;
            const fastPasteIndex = index < 9 ? index + 1 : null;

            return (
              <div
                key={item.id}
                className={`panel-item ${isSelected ? "selected" : ""}`}
                onClick={() => handlePaste(item.id)}
                onMouseEnter={() => setSelectedIndex(index)}
              >
                <div className="item-badge">
                  {fastPasteIndex ? (
                    <span className="badge-num">{fastPasteIndex}</span>
                  ) : (
                    <span className="badge-dot">•</span>
                  )}
                </div>
                <div className="item-content">
                  <div className="item-text">{item.content}</div>
                </div>
                <div className="item-meta">
                  <span className="item-len">{item.content.length} 字符</span>
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
};

export default App;
