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

/**
 * 常用短语实体模型 (遵循 CONTEXT.md 与工单 #8)
 */
export interface Snippet {
  id: number;
  title: string;
  content: string;
  shortcut: string;
  created_at: number;
  updated_at: number;
}

/**
 * 队列连贴单项数据结构 (工单 #9)
 */
export interface QueueItem {
  id: number;
  content: string;
  entry_type: string;
}

/**
 * 队列连贴当前全局状态视图 (工单 #9)
 */
export interface QueueStatus {
  is_active: boolean;
  count: number;
  items: QueueItem[];
}

/**
 * 顶部激活 Tab 模式
 */
export type ActiveTab = "history" | "snippets";

/**
 * 列表统一直观展示项模型
 */
export interface DisplayItem {
  id: number;
  isSnippet: boolean;
  content: string;
  title?: string;
  shortcut?: string;
  entry_type: string; // "text" | "image" | "snippet"
  created_at: number;
  is_pinned: boolean;
  rawSnippet?: Snippet;
  rawEntry?: ClipboardEntry;
}

/**
 * 图片多媒体元数据与 Base64 视图模型
 */
interface ImageDetail {
  data_url: string;
  width: number;
  height: number;
  file_size: number;
}

/**
 * 文件大小人性化格式化函数
 */
function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
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
  | "json_minify"
  | "ocr";

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

const IMAGE_ACTIONS: ActionItem[] = [
  { key: "ocr", label: "提取文字 (OCR)", description: "利用系统原生离线 OCR 识别中英文字符", icon: "🔍", hotkey: "1" },
];

export const App: React.FC = () => {
  const [activeTab, setActiveTab] = useState<ActiveTab>("history");
  const [query, setQuery] = useState<string>("");
  const [displayItems, setDisplayItems] = useState<DisplayItem[]>([]);
  const [selectedIndex, setSelectedIndex] = useState<number>(0);
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const [actionPaletteOpen, setActionPaletteOpen] = useState<boolean>(false);
  const [actionSelectedIndex, setActionSelectedIndex] = useState<number>(0);
  const [transformError, setTransformError] = useState<string | null>(null);
  const [imageDetails, setImageDetails] = useState<Record<string, ImageDetail>>({});
  const [previewModalOpen, setPreviewModalOpen] = useState<boolean>(false);
  const [ocrTextMap, setOcrTextMap] = useState<Record<number, string>>({});
  const [ocrLoading, setOcrLoading] = useState<boolean>(false);

  // 多选状态 (工单 #9 AC-4)
  const [selectedIds, setSelectedIds] = useState<number[]>([]);

  // 队列连贴状态 (工单 #9 AC-1 ~ AC-3)
  const [queueStatus, setQueueStatus] = useState<QueueStatus>({
    is_active: false,
    count: 0,
    items: [],
  });

  // 常用短语编辑模态框与表单状态 (AC-1)
  const [snippetModalOpen, setSnippetModalOpen] = useState<boolean>(false);
  const [editingSnippet, setEditingSnippet] = useState<Snippet | null>(null);
  const [snippetTitle, setSnippetTitle] = useState<string>("");
  const [snippetShortcut, setSnippetShortcut] = useState<string>("");
  const [snippetContent, setSnippetContent] = useState<string>("");
  const [snippetFormError, setSnippetFormError] = useState<string | null>(null);

  const inputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  /**
   * 加载或检索列表数据（支持剪贴板历史与常用短语独立 Tab 及混合模式）
   */
  const loadData = useCallback(async (tab: ActiveTab, searchQuery: string) => {
    try {
      const q = searchQuery.trim();

      // 1. 独立短语 Tab 模式 (AC-1)
      if (tab === "snippets") {
        let snippetsList: Snippet[];
        if (q.length === 0) {
          snippetsList = await invoke<Snippet[]>("get_snippets");
        } else {
          snippetsList = await invoke<Snippet[]>("search_snippets", { query: q });
        }
        setDisplayItems(
          snippetsList.map((s) => ({
            id: s.id,
            isSnippet: true,
            content: s.content,
            title: s.title,
            shortcut: s.shortcut,
            entry_type: "snippet",
            created_at: s.updated_at,
            is_pinned: false,
            rawSnippet: s,
          }))
        );
        setSelectedIndex(0);
        return;
      }

      // 2. 剪贴板历史 Tab 下以 / 开头触发短语快速搜索 (AC-3)
      if (q.startsWith("/")) {
        const snippetsList = await invoke<Snippet[]>("search_snippets", { query: q });
        setDisplayItems(
          snippetsList.map((s) => ({
            id: s.id,
            isSnippet: true,
            content: s.content,
            title: s.title,
            shortcut: s.shortcut,
            entry_type: "snippet",
            created_at: s.updated_at,
            is_pinned: false,
            rawSnippet: s,
          }))
        );
        setSelectedIndex(0);
        return;
      }

      // 3. 常规剪贴板历史拉取
      let history: ClipboardEntry[];
      if (q.length === 0) {
        history = await invoke<ClipboardEntry[]>("get_history", { limit: 50 });
      } else {
        history = await invoke<ClipboardEntry[]>("search_history", { query: q, limit: 50 });
      }

      // 若有关键词搜索，同时混合检索短语并置顶高亮微标展示 (AC-3)
      let matchedSnippets: Snippet[] = [];
      if (q.length > 0) {
        try {
          matchedSnippets = await invoke<Snippet[]>("search_snippets", { query: q });
        } catch {
          // ignore
        }
      }

      const snippetItems: DisplayItem[] = matchedSnippets.map((s) => ({
        id: s.id,
        isSnippet: true,
        content: s.content,
        title: s.title,
        shortcut: s.shortcut,
        entry_type: "snippet",
        created_at: s.updated_at,
        is_pinned: false,
        rawSnippet: s,
      }));

      const historyItems: DisplayItem[] = history.map((item) => ({
        id: item.id,
        isSnippet: false,
        content: item.content,
        entry_type: item.entry_type,
        created_at: item.created_at,
        is_pinned: item.is_pinned,
        rawEntry: item,
      }));

      setDisplayItems([...snippetItems, ...historyItems]);
      setSelectedIndex(0);

      // 并行批量拉取图片缩略图 Base64 详情并缓存
      const imageItems = history.filter((item) => item.entry_type === "image");
      for (const img of imageItems) {
        invoke<ImageDetail>("get_image_detail", { hash: img.content })
          .then((detail) => {
            setImageDetails((prev) => ({ ...prev, [img.content]: detail }));
          })
          .catch((err) => console.error("加载图片详情失败:", err));
      }
    } catch (err) {
      console.error("加载列表数据失败:", err);
    }
  }, []);

  /**
   * 触发指定条目的极速回填
   */
  const handlePaste = useCallback(async (id: number) => {
    try {
      await invoke("paste_entry", { id });
      setPreviewModalOpen(false);
    } catch (err) {
      console.error("回填剪贴板条目失败:", err);
    }
  }, []);

  /**
   * 触发常用短语的极速回填与模板动态变量解析 (AC-2)
   */
  const handlePasteSnippet = useCallback(async (id: number) => {
    try {
      await invoke("paste_snippet", { id });
      setPreviewModalOpen(false);
    } catch (err) {
      console.error("回填常用短语失败:", err);
    }
  }, []);

  /**
   * 切换单个条目的多选勾选状态 (AC-4)
   */
  const handleToggleSelectItem = useCallback((id: number, e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    setSelectedIds((prev) =>
      prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id]
    );
  }, []);

  /**
   * 多选条目换行合并回填 (AC-4)
   */
  const handlePasteMultiple = useCallback(async (ids: number[]) => {
    if (ids.length === 0) return;
    try {
      await invoke("paste_multiple_entries", { ids, separator: "\n" });
      setSelectedIds([]);
      setPreviewModalOpen(false);
    } catch (err) {
      console.error("多选合并回填失败:", err);
    }
  }, []);

  /**
   * 切换连贴收集模式 (AC-1)
   */
  const handleTogglePasteQueue = useCallback(async () => {
    try {
      const status = await invoke<QueueStatus>("toggle_paste_queue");
      setQueueStatus(status);
    } catch (err) {
      console.error("切换连贴模式失败:", err);
    }
  }, []);

  /**
   * 停止连贴模式并清空队列 (AC-3)
   */
  const handleStopPasteQueue = useCallback(async () => {
    try {
      const status = await invoke<QueueStatus>("clear_paste_queue");
      setQueueStatus(status);
    } catch (err) {
      console.error("停止连贴模式失败:", err);
    }
  }, []);

  /**
   * 连贴队列头部出队并回填 (AC-2)
   */
  const handlePasteQueuePop = useCallback(async () => {
    try {
      await invoke("paste_queue_pop");
    } catch (err) {
      console.error("连贴出队回填失败:", err);
    }
  }, []);

  /**
   * 触发自定义文本（如 OCR 提取文本）的极速回填
   */
  const handlePasteCustomText = useCallback(async (text: string) => {
    try {
      await invoke("paste_custom_text", { text });
      setPreviewModalOpen(false);
    } catch (err) {
      console.error("回填自定义文本失败:", err);
    }
  }, []);

  /**
   * 触发指定图片条目的原生离线 OCR 识别
   */
  const handleOcr = useCallback(
    async (id: number) => {
      setOcrLoading(true);
      setTransformError(null);
      try {
        const recognized = await invoke<string>("ocr_image_entry", { id });
        setOcrTextMap((prev) => ({ ...prev, [id]: recognized }));
        setPreviewModalOpen(true);
        loadData(activeTab, query);
      } catch (err: unknown) {
        const msg = typeof err === "string" ? err : "文字提取失败";
        setTransformError(msg);
        setTimeout(() => setTransformError(null), 3000);
      } finally {
        setOcrLoading(false);
        setActionPaletteOpen(false);
      }
    },
    [loadData, activeTab, query]
  );

  /**
   * 触发指定条目的动作转换与极速回填
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
   * 统一执行选中的动作项
   */
  const executeAction = useCallback(
    (item: ClipboardEntry, actionKey: ActionKey) => {
      if (actionKey === "ocr") {
        handleOcr(item.id);
      } else {
        handleTransformAndPaste(item.id, actionKey);
      }
    },
    [handleOcr, handleTransformAndPaste]
  );

  /**
   * 强制以纯文本格式回填当前条目 (Shift + Enter 专用)
   */
  const handlePastePlain = useCallback(async (id: number) => {
    try {
      await invoke("paste_plain_entry", { id });
      setPreviewModalOpen(false);
    } catch (err) {
      console.error("纯文本回填失败:", err);
    }
  }, []);

  /**
   * 切换指定条目的置顶固定状态 (Pin / Unpin)
   */
  const handleTogglePin = useCallback(
    async (id: number, e?: React.MouseEvent) => {
      if (e) {
        e.stopPropagation();
      }
      try {
        await invoke("toggle_pin", { id });
        loadData(activeTab, query);
      } catch (err) {
        console.error("切换置顶状态失败:", err);
      }
    },
    [loadData, activeTab, query]
  );

  /**
   * 打开新建常用短语弹窗
   */
  const handleOpenCreateSnippet = useCallback(() => {
    setEditingSnippet(null);
    setSnippetTitle("");
    setSnippetShortcut("");
    setSnippetContent("");
    setSnippetFormError(null);
    setSnippetModalOpen(true);
  }, []);

  /**
   * 打开编辑常用短语弹窗
   */
  const handleOpenEditSnippet = useCallback((snippet: Snippet, e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    setEditingSnippet(snippet);
    setSnippetTitle(snippet.title);
    setSnippetShortcut(snippet.shortcut);
    setSnippetContent(snippet.content);
    setSnippetFormError(null);
    setSnippetModalOpen(true);
  }, []);

  /**
   * 删除常用短语 (AC-1)
   */
  const handleDeleteSnippet = useCallback(
    async (id: number, e?: React.MouseEvent) => {
      if (e) e.stopPropagation();
      try {
        await invoke("delete_snippet", { id });
        loadData(activeTab, query);
      } catch (err) {
        console.error("删除常用短语失败:", err);
      }
    },
    [activeTab, query, loadData]
  );

  /**
   * 保存或更新常用短语模板 (AC-1)
   */
  const handleSaveSnippet = useCallback(async () => {
    if (!snippetTitle.trim()) {
      setSnippetFormError("短语标题不能为空");
      return;
    }
    if (!snippetContent.trim()) {
      setSnippetFormError("短语模板内容不能为空");
      return;
    }
    try {
      await invoke("save_snippet", {
        id: editingSnippet ? editingSnippet.id : null,
        title: snippetTitle.trim(),
        content: snippetContent,
        shortcut: snippetShortcut.trim().replace(/^\/+/, ""),
      });
      setSnippetModalOpen(false);
      loadData(activeTab, query);
    } catch (err: unknown) {
      const msg = typeof err === "string" ? err : "保存短语失败";
      setSnippetFormError(msg);
    }
  }, [editingSnippet, snippetTitle, snippetContent, snippetShortcut, activeTab, query, loadData]);

  /**
   * 快捷向模板内容光标处插入动态占位符变量 (AC-2)
   */
  const insertPlaceholder = useCallback((ph: string) => {
    if (textareaRef.current) {
      const el = textareaRef.current;
      const start = el.selectionStart;
      const end = el.selectionEnd;
      const val = el.value;
      const next = val.substring(0, start) + ph + val.substring(end);
      setSnippetContent(next);
      setTimeout(() => {
        el.focus();
        el.setSelectionRange(start + ph.length, start + ph.length);
      }, 10);
    } else {
      setSnippetContent((prev) => prev + ph);
    }
  }, []);

  /**
   * 主动隐藏悬浮面板
   */
  const handleClose = useCallback(async () => {
    setActionPaletteOpen(false);
    setPreviewModalOpen(false);
    setSnippetModalOpen(false);
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
    loadData(activeTab, val);
  };

  // Tab 模式切换
  const handleTabChange = (newTab: ActiveTab) => {
    setActiveTab(newTab);
    loadData(newTab, query);
    inputRef.current?.focus();
  };

  // 监听后端广播事件与窗口唤起
  useEffect(() => {
    loadData(activeTab, "");

    // 1. 获取初始连贴状态 (AC-1)
    invoke<QueueStatus>("get_paste_queue_status")
      .then(setQueueStatus)
      .catch((err) => console.error("获取连贴状态失败:", err));

    // 2. 监听系统剪贴板更新事件
    const unlistenClipboard = listen<ClipboardEntry>("clipboard-changed", () => {
      loadData(activeTab, query);
    });

    // 3. 监听队列连贴状态变更广播 (AC-1 ~ AC-3)
    const unlistenQueue = listen<QueueStatus>("paste-queue-changed", (event) => {
      setQueueStatus(event.payload);
    });

    // 4. 监听窗口唤起展示事件 (初始化焦点与清空历史)
    const unlistenPanelShown = listen("panel-shown", () => {
      setQuery("");
      setActiveTab("history");
      loadData("history", "");
      setSelectedIndex(0);
      setSelectedIds([]);
      setActionPaletteOpen(false);
      setPreviewModalOpen(false);
      setSnippetModalOpen(false);
      setTimeout(() => {
        inputRef.current?.focus();
        inputRef.current?.select();
      }, 20);
    });

    // 5. 页面失焦无感自隐防御
    const handleBlur = () => {
      handleClose();
    };
    window.addEventListener("blur", handleBlur);

    return () => {
      unlistenClipboard.then((f) => f());
      unlistenQueue.then((f) => f());
      unlistenPanelShown.then((f) => f());
      window.removeEventListener("blur", handleBlur);
    };
  }, [loadData, activeTab, query, handleClose]);

  // 全局键盘导航处理
  // 全局键盘导航处理
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // 1. 输入法合成状态保护：正在输入中文拼音时，Enter 仅用于确认上屏，绝不触发粘贴
      if (e.isComposing) {
        return;
      }

      // 2. 短语编辑模态框处于开启态时的键盘路由
      if (snippetModalOpen) {
        if (e.key === "Escape") {
          e.preventDefault();
          setSnippetModalOpen(false);
          return;
        }
        // 模态框内打字编辑时不拦截其它输入
        return;
      }

      // 3. 大图放大预览模态框开启时的键盘路由
      if (previewModalOpen) {
        if (e.key === "Escape" || e.key === " ") {
          e.preventDefault();
          setPreviewModalOpen(false);
          return;
        }
        if (e.key === "Enter") {
          e.preventDefault();
          const current = displayItems[selectedIndex];
          if (current) {
            handlePaste(current.id);
          }
          return;
        }
        e.preventDefault();
        return;
      }

      // 4. Action Palette 处于激活态时的键盘路由
      if (actionPaletteOpen) {
        const current = displayItems[selectedIndex];
        const activeActions = current?.entry_type === "image" ? IMAGE_ACTIONS : TRANSFORM_ACTIONS;

        if (e.key === "Escape" || (e.ctrlKey && (e.key === "k" || e.key === "K")) || e.key === "Tab") {
          e.preventDefault();
          setActionPaletteOpen(false);
          return;
        }

        if (e.key === "ArrowDown") {
          e.preventDefault();
          setActionSelectedIndex((prev) => (prev < activeActions.length - 1 ? prev + 1 : 0));
          return;
        }

        if (e.key === "ArrowUp") {
          e.preventDefault();
          setActionSelectedIndex((prev) => (prev > 0 ? prev - 1 : activeActions.length - 1));
          return;
        }

        if (e.key === "Enter") {
          e.preventDefault();
          const action = activeActions[actionSelectedIndex];
          if (current && current.rawEntry && action) {
            executeAction(current.rawEntry, action.key);
          }
          return;
        }

        // 数字键快捷触发对应动作 (1~8)
        if (e.key >= "1" && e.key <= String(activeActions.length)) {
          const actionIdx = parseInt(e.key, 10) - 1;
          const action = activeActions[actionIdx];
          if (current && current.rawEntry && action) {
            e.preventDefault();
            executeAction(current.rawEntry, action.key);
            return;
          }
        }

        e.preventDefault();
        e.stopPropagation();
        return;
      }

      // 5. 主列表状态下的按键调度
      // Esc: 优先取消多选；若未多选则瞬间无感自隐并释放焦点 (AC-4)
      if (e.key === "Escape") {
        e.preventDefault();
        if (selectedIds.length > 0) {
          setSelectedIds([]);
          return;
        }
        handleClose();
        return;
      }

      // Ctrl + 1 / Ctrl + 2: 切换 Tab 模式
      if (e.ctrlKey && e.key === "1") {
        e.preventDefault();
        handleTabChange("history");
        return;
      }
      if (e.ctrlKey && e.key === "2") {
        e.preventDefault();
        handleTabChange("snippets");
        return;
      }

      // Ctrl + N: 新建常用短语
      if (e.ctrlKey && (e.key === "n" || e.key === "N")) {
        e.preventDefault();
        handleOpenCreateSnippet();
        return;
      }

      // Tab 或 Ctrl+K: 唤出 Action Palette 动作浮层 (仅对历史条目生效)
      if ((e.ctrlKey && (e.key === "k" || e.key === "K")) || e.key === "Tab") {
        const current = displayItems[selectedIndex];
        if (current && !current.isSnippet) {
          e.preventDefault();
          setActionSelectedIndex(0);
          setActionPaletteOpen(true);
          return;
        }
      }

      // Space 或 Alt+Space: 图片大图放大预览
      const isSpaceTrigger = (e.key === " " && (document.activeElement !== inputRef.current || query.length === 0)) || (e.altKey && e.key === " ");
      if (isSpaceTrigger) {
        const current = displayItems[selectedIndex];
        if (current && current.entry_type === "image") {
          e.preventDefault();
          setPreviewModalOpen(true);
          return;
        }
      }

      // Shift + Enter: 强制纯文本格式极速回填 (仅普通文本条目生效)
      if (e.shiftKey && e.key === "Enter") {
        e.preventDefault();
        const current = displayItems[selectedIndex];
        if (current && !current.isSnippet && current.entry_type === "text") {
          handlePastePlain(current.id);
        }
        return;
      }

      // Alt + P 快捷切换当前选中项置顶 (仅历史条目生效)
      if (e.altKey && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        const current = displayItems[selectedIndex];
        if (current && !current.isSnippet) {
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
        const targetItem = displayItems[num - 1];
        if (targetItem) {
          e.preventDefault();
          if (targetItem.isSnippet) {
            handlePasteSnippet(targetItem.id);
          } else {
            handlePaste(targetItem.id);
          }
          return;
        }
      }

      // Shift + 上下键扩展多选范围 (工单 #9 AC-4)
      if (e.shiftKey && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
        e.preventDefault();
        const nextIndex =
          e.key === "ArrowDown"
            ? Math.min(selectedIndex + 1, displayItems.length - 1)
            : Math.max(selectedIndex - 1, 0);
        setSelectedIndex(nextIndex);
        const targetItem = displayItems[nextIndex];
        if (targetItem && !targetItem.isSnippet) {
          setSelectedIds((prev) => {
            const currentItem = displayItems[selectedIndex];
            const set = new Set(prev);
            if (currentItem && !currentItem.isSnippet) set.add(currentItem.id);
            set.add(targetItem.id);
            return Array.from(set);
          });
        }
        return;
      }

      // 上下方向键导航
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((prev) => (prev < displayItems.length - 1 ? prev + 1 : prev));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((prev) => (prev > 0 ? prev - 1 : 0));
      } else if (e.key === "Enter") {
        // 多选模式下按回车以换行符合并粘贴至目标窗口 (AC-4)
        if (selectedIds.length > 1) {
          e.preventDefault();
          handlePasteMultiple(selectedIds);
          return;
        }
        if (selectedIds.length === 1) {
          e.preventDefault();
          handlePaste(selectedIds[0]);
          setSelectedIds([]);
          return;
        }
        // 回车极速回填当前高亮条目 (AC-2 & AC-3)
        e.preventDefault();
        const current = displayItems[selectedIndex];
        if (current) {
          if (current.isSnippet) {
            handlePasteSnippet(current.id);
          } else {
            handlePaste(current.id);
          }
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    displayItems,
    selectedIndex,
    selectedIds,
    query,
    snippetModalOpen,
    actionPaletteOpen,
    actionSelectedIndex,
    previewModalOpen,
    handleClose,
    handlePaste,
    handlePastePlain,
    handlePasteSnippet,
    handlePasteMultiple,
    handleTransformAndPaste,
    handleTogglePin,
    handleOpenCreateSnippet,
    handleTabChange,
    executeAction,
  ]);

  const isHudView = typeof window !== "undefined" && window.location.search.includes("view=hud");

  if (isHudView) {
    return (
      <div className="capsule-hud-standalone">
        <div className="capsule-hud-main">
          <div className="capsule-hud-header">
            <span className="capsule-pulse-dot" />
            <span className="capsule-hud-title">📥 连贴模式</span>
            <span className="capsule-hud-count">{queueStatus.count} 项</span>
          </div>
          <div className="capsule-hud-sub">
            {queueStatus.count > 0 ? (
              <span>目标窗口连按 <kbd>Ctrl+V</kbd> 依次回填 (FIFO)</span>
            ) : (
              <span>等待复制入队，清空自动退出</span>
            )}
          </div>
        </div>
        <div className="capsule-hud-buttons">
          {queueStatus.count > 0 && (
            <button
              className="capsule-action-btn pop"
              onClick={handlePasteQueuePop}
              title="手动出队回填下一项"
            >
              回填
            </button>
          )}
          <button
            className="capsule-action-btn stop"
            onClick={handleStopPasteQueue}
            title="退出连贴模式 (Alt+Shift+C)"
          >
            退出
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="panel-container">
      <header className="panel-header" data-tauri-drag-region>
        <div className="tab-bar">
          <div className="tab-group">
            <button
              className={`tab-btn ${activeTab === "history" ? "active" : ""}`}
              onClick={() => handleTabChange("history")}
            >
              📋 剪贴板历史
            </button>
            <button
              className={`tab-btn ${activeTab === "snippets" ? "active" : ""}`}
              onClick={() => handleTabChange("snippets")}
            >
              ⚡ 常用短语
            </button>
          </div>
          <div className="tab-actions">
            <button
              className={`queue-toggle-btn ${queueStatus.is_active ? "active" : ""}`}
              onClick={handleTogglePasteQueue}
              title="切换队列连贴模式 (Alt+Shift+C)"
            >
              📥 {queueStatus.is_active ? `连贴中 (${queueStatus.count})` : "连贴收集 (Alt+Shift+C)"}
            </button>
            {activeTab === "snippets" && (
              <button className="new-snippet-btn" onClick={handleOpenCreateSnippet} title="新建短语模板 (Ctrl+N)">
                + 新建短语
              </button>
            )}
          </div>
        </div>

        <div className="search-bar">
          <span className="search-icon">🔍</span>
          <input
            ref={inputRef}
            type="text"
            className="search-input"
            value={query}
            onChange={handleQueryChange}
            placeholder={
              activeTab === "snippets"
                ? "搜索短语（支持标题、/缩写如 /meet 或内容）..."
                : "搜索剪贴板（输入 / 快速唤起常用短语，支持中文拼音）..."
            }
            autoFocus
          />
        </div>
        <div className="shortcut-hints">
          {activeTab === "snippets" ? (
            <>
              <span className="hint-tag"><kbd>1~9</kbd> / <kbd>↵</kbd> 回填</span>
              <span className="hint-tag"><kbd>Ctrl+N</kbd> 新建</span>
              <span className="hint-tag"><kbd>Ctrl+1</kbd> 历史</span>
              <span className="hint-tag"><kbd>Esc</kbd> 自隐</span>
            </>
          ) : (
            <>
              <span className="hint-tag"><kbd>Alt+Shift+C</kbd> 连贴</span>
              <span className="hint-tag"><kbd>Shift+↑↓</kbd> 多选</span>
              <span className="hint-tag"><kbd>Tab</kbd> / <kbd>Ctrl+K</kbd> 动作</span>
              <span className="hint-tag"><kbd>1~9</kbd> 回填</span>
              <span className="hint-tag"><kbd>Alt+P</kbd> 置顶</span>
              <span className="hint-tag"><kbd>Esc</kbd> 自隐</span>
            </>
          )}
        </div>
      </header>

      <div className="panel-list" ref={listRef}>
        {displayItems.length === 0 ? (
          <div className="empty-state">
            <p>{query ? "未找到匹配条目" : activeTab === "snippets" ? "暂无常用短语模板" : "暂无剪贴板历史记录"}</p>
            <span className="empty-sub">
              {query
                ? "尝试更换拼音首字母简拼或模糊关键词"
                : activeTab === "snippets"
                ? "点击上方 [+ 新建短语] 预置高频常用回复或模板"
                : "复制任意文本或截屏图片后将自动捕获并在此显示"}
            </span>
          </div>
        ) : (
          displayItems.map((item, index) => {
            const isSelected = index === selectedIndex;
            const fastPasteIndex = index < 9 ? index + 1 : null;

            // 1. 常用短语模板卡片 (AC-1 & AC-3)
            if (item.isSnippet) {
              return (
                <div
                  key={`snippet-${item.id}`}
                  className={`panel-item snippet-item ${isSelected ? "selected" : ""}`}
                  onClick={() => handlePasteSnippet(item.id)}
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
                    {fastPasteIndex ? (
                      <span className="badge-num">{fastPasteIndex}</span>
                    ) : (
                      <span className="badge-dot">•</span>
                    )}
                  </div>
                  <div className="item-content">
                    <div className="item-text-line">
                      <span className="tag-snippet">[短语]</span>
                      {item.shortcut && <span className="tag-shortcut">/{item.shortcut}</span>}
                      <span className="snippet-title-text">{item.title}</span>
                    </div>
                    <div className="snippet-preview-text">
                      {item.content.length > 90 ? item.content.slice(0, 90) + "..." : item.content}
                    </div>
                  </div>
                  <div className="item-meta">
                    {item.rawSnippet && (
                      <>
                        <button
                          className="snippet-action-btn"
                          onClick={(e) => handleOpenEditSnippet(item.rawSnippet!, e)}
                          title="编辑短语"
                        >
                          ✏️
                        </button>
                        <button
                          className="snippet-action-btn del"
                          onClick={(e) => handleDeleteSnippet(item.id, e)}
                          title="删除短语"
                        >
                          🗑️
                        </button>
                      </>
                    )}
                    <span className="item-len">模板</span>
                  </div>
                </div>
              );
            }

            // 2. 图片多媒体卡片 (Issue #7)
            if (item.entry_type === "image") {
              const imgDetail = imageDetails[item.content];
              const ocrText = ocrTextMap[item.id];
              const isItemMultiSelected = selectedIds.includes(item.id);
              return (
                <div
                  key={`entry-${item.id}`}
                  className={`panel-item image-item ${isSelected ? "selected" : ""} ${item.is_pinned ? "pinned" : ""} ${isItemMultiSelected ? "multi-selected" : ""}`}
                  onClick={(e) => {
                    if (e.ctrlKey || e.metaKey || e.shiftKey) {
                      handleToggleSelectItem(item.id, e);
                    } else if (selectedIds.length > 0) {
                      handleToggleSelectItem(item.id, e);
                    } else {
                      handlePaste(item.id);
                    }
                  }}
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
                  <div
                    className={`item-checkbox ${isItemMultiSelected ? "checked" : ""}`}
                    onClick={(e) => handleToggleSelectItem(item.id, e)}
                    title="勾选此项参与多选合并粘贴"
                  >
                    {isItemMultiSelected ? "✓" : ""}
                  </div>
                  <div className="item-badge">
                    {item.is_pinned ? (
                      <span className="badge-pin" title="置顶条目">📌</span>
                    ) : fastPasteIndex ? (
                      <span className="badge-num">{fastPasteIndex}</span>
                    ) : (
                      <span className="badge-dot">•</span>
                    )}
                  </div>
                  <div className="item-image-wrapper">
                    {imgDetail ? (
                      <img
                        src={imgDetail.data_url}
                        alt="缩略图"
                        className="item-thumbnail"
                        onClick={(e) => {
                          e.stopPropagation();
                          setSelectedIndex(index);
                          setPreviewModalOpen(true);
                        }}
                        title="点击或按空格放大预览"
                      />
                    ) : (
                      <div className="item-thumbnail placeholder">🖼️</div>
                    )}
                    <div className="item-image-meta">
                      <div className="item-image-title">
                        <span className="tag-image">🖼️ 图片</span>
                        {ocrText && <span className="tag-ocr">OCR 提取</span>}
                        <span>{imgDetail ? `${imgDetail.width} × ${imgDetail.height}` : "位图数据"}</span>
                      </div>
                      <div className="item-image-dims">
                        {ocrText ? `提取文本: ${ocrText.slice(0, 36)}...` : `哈希: ${item.content.slice(0, 16)}... (按空格大图预览)`}
                      </div>
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
                      {imgDetail ? formatBytes(imgDetail.file_size) : "图片"}
                    </span>
                  </div>
                </div>
              );
            }

            // 3. 纯文本条目卡片
            const isLargeText = item.content.length > LARGE_TEXT_THRESHOLD;
            const { displayText: maskedText, isSensitive } = maskSensitiveContent(item.content);
            const isHovered = index === hoveredIndex;
            const isRevealed = isSensitive && isHovered;
            const activeText = isSensitive && !isRevealed ? maskedText : item.content;
            const displayText = isLargeText ? activeText.slice(0, 300) + "..." : activeText;
            const isItemMultiSelected = selectedIds.includes(item.id);

            return (
              <div
                key={`entry-${item.id}`}
                className={`panel-item ${isSelected ? "selected" : ""} ${item.is_pinned ? "pinned" : ""} ${isItemMultiSelected ? "multi-selected" : ""}`}
                onClick={(e) => {
                  if (e.ctrlKey || e.metaKey || e.shiftKey) {
                    handleToggleSelectItem(item.id, e);
                  } else if (selectedIds.length > 0) {
                    handleToggleSelectItem(item.id, e);
                  } else {
                    handlePaste(item.id);
                  }
                }}
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
                <div
                  className={`item-checkbox ${isItemMultiSelected ? "checked" : ""}`}
                  onClick={(e) => handleToggleSelectItem(item.id, e)}
                  title="勾选此项参与多选合并粘贴"
                >
                  {isItemMultiSelected ? "✓" : ""}
                </div>
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

      {/* 多选合并回填浮动工具栏 (AC-4) */}
      {selectedIds.length > 1 && (
        <div className="multi-select-toolbar">
          <div className="multi-select-info">
            <span className="multi-select-badge">{selectedIds.length}</span>
            <span>已选择 {selectedIds.length} 项 (按 Enter 换行合并粘贴，Esc 取消选择)</span>
          </div>
          <div className="multi-select-actions">
            <button
              className="multi-btn primary"
              onClick={() => handlePasteMultiple(selectedIds)}
              title="按换行符合并粘贴到当前活动窗口"
            >
              ↵ 换行合并粘贴
            </button>
            <button
              className="multi-btn"
              onClick={() => setSelectedIds([])}
              title="取消多选 (Esc)"
            >
              取消
            </button>
          </div>
        </div>
      )}

      {/* 队列连贴屏幕右下角计数胶囊 (Capsule HUD - AC-1, AC-2, AC-3) */}
      {queueStatus.is_active && (
        <div className="capsule-hud">
          <div className="capsule-hud-main">
            <div className="capsule-hud-header">
              <span className="capsule-pulse-dot" />
              <span className="capsule-hud-title">📥 连贴模式</span>
              <span className="capsule-hud-count">{queueStatus.count} 项</span>
            </div>
            <div className="capsule-hud-sub">
              {queueStatus.count > 0 ? (
                <span>目标窗口连按 <kbd>Ctrl+V</kbd> 依次回填 (FIFO)</span>
              ) : (
                <span>等待复制入队，清空自动退出</span>
              )}
            </div>
          </div>
          <div className="capsule-hud-buttons">
            {queueStatus.count > 0 && (
              <button
                className="capsule-action-btn pop"
                onClick={handlePasteQueuePop}
                title="手动回填下一项"
              >
                回填
              </button>
            )}
            <button
              className="capsule-action-btn stop"
              onClick={handleStopPasteQueue}
              title="退出连贴模式 (Alt+Shift+C)"
            >
              退出
            </button>
          </div>
        </div>
      )}

      {actionPaletteOpen && displayItems[selectedIndex]?.rawEntry && (
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
                  目标条目: {displayItems[selectedIndex].entry_type === "image" ? `[图片] ${displayItems[selectedIndex].content.slice(0, 32)}...` : `${displayItems[selectedIndex].content.slice(0, 48).replace(/\n/g, " ")}...`}
                </div>
              )}
            </div>
            <div className="palette-list">
              {(displayItems[selectedIndex].entry_type === "image" ? IMAGE_ACTIONS : TRANSFORM_ACTIONS).map((action, idx) => {
                const isActionSelected = idx === actionSelectedIndex;
                return (
                  <div
                    key={action.key}
                    className={`palette-item ${isActionSelected ? "selected" : ""}`}
                    onClick={() => {
                      const entry = displayItems[selectedIndex].rawEntry;
                      if (entry) executeAction(entry, action.key);
                    }}
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

      {previewModalOpen && displayItems[selectedIndex]?.entry_type === "image" && (
        <div
          className="image-preview-overlay"
          onClick={() => setPreviewModalOpen(false)}
        >
          <div
            className="image-preview-modal"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="image-preview-header">
              <div className="image-preview-title">
                <span>🖼️ 图片大图放大预览</span>
              </div>
              <button
                className="image-preview-close"
                onClick={() => setPreviewModalOpen(false)}
              >
                ✕
              </button>
            </div>
            <div className="image-preview-body">
              {imageDetails[displayItems[selectedIndex].content] && (
                <img
                  src={imageDetails[displayItems[selectedIndex].content].data_url}
                  alt="大图预览"
                  className="image-preview-img"
                />
              )}
              {ocrTextMap[displayItems[selectedIndex].id] && (
                <div className="ocr-result-container">
                  <div className="ocr-header">
                    <span>🔍 离线 OCR 提取文本：</span>
                    <button
                      className="preview-action-btn"
                      onClick={() => handlePasteCustomText(ocrTextMap[displayItems[selectedIndex].id])}
                      title="粘贴提取文本"
                    >
                      📄 回填文本
                    </button>
                  </div>
                  <div className="ocr-text-view">
                    {ocrTextMap[displayItems[selectedIndex].id]}
                  </div>
                </div>
              )}
            </div>
            <div className="image-preview-footer">
              <div className="image-preview-info">
                {imageDetails[displayItems[selectedIndex].content] && (
                  <>
                    <span>尺寸: {imageDetails[displayItems[selectedIndex].content].width} × {imageDetails[displayItems[selectedIndex].content].height} 像素</span>
                    <span>大小: {formatBytes(imageDetails[displayItems[selectedIndex].content].file_size)}</span>
                  </>
                )}
              </div>
              <div className="image-preview-actions">
                <button
                  className="preview-action-btn"
                  onClick={() => handleOcr(displayItems[selectedIndex].id)}
                  disabled={ocrLoading}
                >
                  {ocrLoading ? "⏳ 识别中..." : "🔍 提取文字 (OCR)"}
                </button>
                <button
                  className="preview-action-btn primary"
                  onClick={() => handlePaste(displayItems[selectedIndex].id)}
                >
                  ↵ 极速回填图片
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {snippetModalOpen && (
        <div className="snippet-modal-overlay" onClick={() => setSnippetModalOpen(false)}>
          <div className="snippet-modal" onClick={(e) => e.stopPropagation()}>
            <div className="snippet-modal-header">
              <div className="snippet-modal-title">
                <span>{editingSnippet ? "✏️ 编辑常用短语" : "✨ 新建常用短语"}</span>
              </div>
              <button className="snippet-modal-close" onClick={() => setSnippetModalOpen(false)}>
                ✕
              </button>
            </div>
            <div className="snippet-modal-body">
              {snippetFormError && <div className="modal-error">⚠️ {snippetFormError}</div>}
              <div className="form-group">
                <label className="form-label">短语标题 <span className="req">*</span></label>
                <input
                  type="text"
                  className="form-input"
                  value={snippetTitle}
                  onChange={(e) => setSnippetTitle(e.target.value)}
                  placeholder="例如: 今日站会汇报、常用联系信息"
                  autoFocus
                />
              </div>
              <div className="form-group">
                <label className="form-label">快捷指令 (输入 /{snippetShortcut || "shortcut"} 快速唤起)</label>
                <div className="shortcut-input-wrapper">
                  <span className="shortcut-prefix">/</span>
                  <input
                    type="text"
                    className="form-input shortcut-input"
                    value={snippetShortcut}
                    onChange={(e) => setSnippetShortcut(e.target.value)}
                    placeholder="如 meet、info、ref"
                  />
                </div>
              </div>
              <div className="form-group">
                <div className="form-label-row">
                  <label className="form-label">模板内容 <span className="req">*</span></label>
                  <span className="form-tip">点击插入动态占位符变量</span>
                </div>
                <div className="placeholder-toolbar">
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{current_date}")} title="当前日期 (YYYY-MM-DD)">
                    + &#123;current_date&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{time}")} title="当前时间 (HH:mm:ss)">
                    + &#123;time&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{datetime}")} title="完整时间 (YYYY-MM-DD HH:mm:ss)">
                    + &#123;datetime&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{clipboard}")} title="展开时嵌入当前系统剪贴板文本">
                    + &#123;clipboard&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{year}")} title="年份">
                    + &#123;year&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{month}")} title="月份">
                    + &#123;month&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{day}")} title="日">
                    + &#123;day&#125;
                  </button>
                </div>
                <textarea
                  ref={textareaRef}
                  className="form-textarea"
                  rows={5}
                  value={snippetContent}
                  onChange={(e) => setSnippetContent(e.target.value)}
                  placeholder="请输入短语内容模板。支持嵌入动态占位符，如：&#10;【{current_date} 站会汇报】&#10;1. 昨日进展：&#10;2. 今日计划："
                />
              </div>
            </div>
            <div className="snippet-modal-footer">
              <button className="btn-cancel" onClick={() => setSnippetModalOpen(false)}>
                取消
              </button>
              <button className="btn-primary" onClick={handleSaveSnippet}>
                保存短语
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default App;
