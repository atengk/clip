/**
 * 悬浮剪贴板历史面板，集成拼音模糊搜索框、数字键 1~9 动态极速回填与失焦自隐。
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
  const [query, setQuery] = useState<string>("");
  const [entries, setEntries] = useState<ClipboardEntry[]>([]);
  const [selectedIndex, setSelectedIndex] = useState<number>(0);
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
   * 主动隐藏悬浮面板
   */
  const handleClose = useCallback(async () => {
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
      // 1. Esc: 瞬间无感自隐并释放焦点
      if (e.key === "Escape") {
        e.preventDefault();
        handleClose();
        return;
      }

      // 2. 输入法合成状态保护：正在输入中文拼音时，Enter 仅用于确认上屏，绝不触发粘贴
      if (e.isComposing) {
        return;
      }

      // 3. 动态序号极速回填：Alt + 1~9 强制回填，或输入框无内容/非输入态敲数字键回填
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

      // 4. 上下方向键导航
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
  }, [entries, selectedIndex, query, fetchEntries, handleClose, handlePaste]);

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
          <span className="hint-tag"><kbd>1~9</kbd> / <kbd>Alt+1~9</kbd> 粘贴</span>
          <span className="hint-tag"><kbd>↵</kbd> 回填</span>
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
