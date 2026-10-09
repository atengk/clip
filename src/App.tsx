/**
 * Clip - 桌面悬浮剪贴板历史管理器
 * 人机工效精修版：浅色 Acrylic 视觉、二层紧凑头部、严格 28×28 槽位对齐与键盘流原生闭环。
 *
 * @author Ateng
 * @since 2026-10-07
 */

import React, { useEffect, useState, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { openUrl } from "@tauri-apps/plugin-opener";
import { maskSensitiveContent } from "./utils/privacy";
import { compareSemVer, formatBytes } from "./utils/version";
import { parseColor } from "./utils/color";
import { ReleaseNotesView } from "./components/ReleaseNotesView";
import "./App.css";

/**
 * 当前客户端编译版本号 (SemVer)
 */
export const CURRENT_VERSION = "v1.2.10";

/**
 * 存储状态与磁盘占用摘要信息契约 (遵循 Issue #19)
 */
export interface StorageInfo {
  db_path: string;
  db_size_bytes: number;
  blob_dir: string;
  blob_size_bytes: number;
  total_entries: number;
  total_snippets: number;
}

/**
 * GitHub Release 构件资产契约 (Issue #20)
 */
export interface GitHubReleaseAsset {
  name: string;
  browser_download_url: string;
  size: number;
}

/**
 * GitHub Release 最新发布信息契约
 */
export interface GitHubReleaseInfo {
  tag_name: string;
  name?: string | null;
  body?: string | null;
  published_at?: string | null;
  html_url?: string | null;
  assets?: GitHubReleaseAsset[];
}

/**
 * 敏感凭据临时显隐超时毫秒数
 */
const SENSITIVE_REVEAL_TIMEOUT_MS = 3000;

/**
 * 剪贴板条目数据结构 (遵循 CONTEXT.md)
 */
export interface ClipboardEntry {
  id: number;
  content: string;
  entry_type: string;
  created_at: number;
  is_pinned: boolean;
}

/**
 * 常用短语实体模型 (遵循 CONTEXT.md)
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
 * 队列连贴单项数据结构
 */
export interface QueueItem {
  id: number;
  content: string;
  entry_type: string;
}

/**
 * 队列连贴当前全局状态视图
 */
export interface QueueStatus {
  is_active: boolean;
  count: number;
  items: QueueItem[];
}

/**
 * 隐身无痕模式状态快照
 */
export interface IncognitoStatus {
  is_active: boolean;
  expires_at?: number | null;
  remaining_seconds?: number | null;
}

/**
 * 灾备归档元数据清单
 */
export interface BackupManifest {
  version: string;
  created_at: number;
  entry_count: number;
  snippet_count: number;
  blob_count: number;
}

/**
 * 顶部激活 Tab 模式
 */
export type ActiveTab = "history" | "snippets";

/**
 * 6 大流线型分类过滤类型
 */
export type FilterCategory = "all" | "pinned" | "text" | "image" | "code" | "link";

/**
 * 自动识别项分类
 */
export type DetectedKind = "text" | "sensitive" | "code" | "link" | "image" | "snippet" | "color";

/**
 * 分类中文映射表 (消除未转译英文枚举泄漏)
 */
export const KIND_NAMES: Record<DetectedKind, string> = {
  text: "纯文本",
  sensitive: "脱敏防窥",
  code: "代码片段",
  link: "外链地址",
  image: "位图图像",
  snippet: "常用短语",
  color: "色彩数值",
};

/**
 * 列表统一直观展示项模型
 */
export interface DisplayItem {
  id: number;
  isSnippet: boolean;
  content: string;
  title: string;
  shortcut?: string;
  entry_type: string;
  created_at: number;
  is_pinned: boolean;
  kind: DetectedKind;
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
 * 人性化相对时间显示函数
 */
function formatRelativeTime(timestampMs: number): string {
  const now = Date.now();
  const diffSec = Math.max(0, Math.floor((now - timestampMs) / 1000));
  if (diffSec < 60) return "刚刚";
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)}分钟前`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}小时前`;
  if (diffSec < 172800) return "昨天";
  return `${Math.floor(diffSec / 86400)}天前`;
}

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

interface ActionItem {
  key: ActionKey;
  label: string;
  description: string;
  icon: string;
  hotkey: string;
}

const TRANSFORM_ACTIONS: ActionItem[] = [
  { key: "trim", label: "去除多余空白与换行 (Trim)", description: "剔除首尾空白，折叠连续多行空白", icon: "✂️", hotkey: "1" },
  { key: "plain_text", label: "纯文本清洗粘贴 (Shift+Enter)", description: "剔除所有富文本样式与控制字符，保留原生换行缩进", icon: "📄", hotkey: "2" },
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

/**
 * 转义正则特殊字符
 */
function escapeRegExp(str: string): string {
  return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * 搜索关键词切片高亮渲染
 */
function renderHighlightedText(text: string, query: string): React.ReactNode {
  const trimmed = query.trim();
  if (!trimmed || !text) {
    return text;
  }
  try {
    const escaped = escapeRegExp(trimmed);
    const regex = new RegExp(`(${escaped})`, "gi");
    const parts = text.split(regex);
    if (parts.length <= 1) {
      return text;
    }
    return parts.map((part, index) =>
      regex.test(part) ? (
        <mark key={index} className="search-highlight">
          {part}
        </mark>
      ) : (
        part
      )
    );
  } catch {
    return text;
  }
}

/**
 * 根据内容智能识别条目分类与默认展示标题
 */
function analyzeContent(content: string, isImage: boolean, isSnippet: boolean, snippetTitle?: string): { kind: DetectedKind; title: string } {
  if (isSnippet) {
    return { kind: "snippet", title: snippetTitle || "常用短语模板" };
  }
  if (isImage) {
    return { kind: "image", title: "屏幕截屏 / 位图图像" };
  }
  const { isSensitive } = maskSensitiveContent(content);
  if (isSensitive) {
    return { kind: "sensitive", title: "敏感信息 (脱敏防窥保护)" };
  }
  const trimmed = content.trim();
  const colorInfo = parseColor(trimmed);
  if (colorInfo) {
    return { kind: "color", title: `色彩: ${colorInfo.hex}` };
  }
  if (/^https?:\/\/[^\s]+$/i.test(trimmed)) {
    try {
      const url = new URL(trimmed);
      return { kind: "link", title: `外链: ${url.hostname}` };
    } catch {
      return { kind: "link", title: "外链地址 (URL)" };
    }
  }
  if (
    trimmed.startsWith("{") ||
    trimmed.startsWith("[") ||
    trimmed.startsWith("<!DOCTYPE") ||
    trimmed.startsWith("<html") ||
    trimmed.startsWith("SELECT ") ||
    trimmed.startsWith("CREATE ") ||
    trimmed.startsWith("import ") ||
    trimmed.startsWith("export ") ||
    trimmed.startsWith("const ") ||
    trimmed.startsWith("let ") ||
    trimmed.startsWith("def ") ||
    trimmed.startsWith("fn ") ||
    trimmed.startsWith("class ") ||
    trimmed.startsWith("curl ") ||
    trimmed.startsWith("git ") ||
    trimmed.includes("=>") ||
    trimmed.includes("function")
  ) {
    if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
      return { kind: "code", title: "JSON 数据结构" };
    }
    if (trimmed.startsWith("SELECT ") || trimmed.startsWith("CREATE ")) {
      return { kind: "code", title: "SQL 查询脚本" };
    }
    if (trimmed.startsWith("git ")) {
      return { kind: "code", title: "Git 终端指令" };
    }
    return { kind: "code", title: "代码 / 脚本片段" };
  }
  const firstLine = content.split("\n")[0].trim();
  return { kind: "text", title: firstLine.length > 36 ? firstLine.slice(0, 36) + "..." : firstLine || "纯文本记录" };
}

/**
 * 独立 HUD 胶囊悬浮窗口
 * 彻底物理隔离于主程序之外，杜绝全量数据拉取、DOM/窗口失焦监听与事件死循环
 */
const StandaloneHudView: React.FC = () => {
  const [queueStatus, setQueueStatus] = useState<QueueStatus>({
    is_active: false,
    count: 0,
    items: [],
  });

  useEffect(() => {
    invoke<QueueStatus>("get_paste_queue_status")
      .then(setQueueStatus)
      .catch((err) => console.error("获取连贴状态失败:", err));

    const unlistenQueue = listen<QueueStatus>("paste-queue-changed", (event) => {
      setQueueStatus(event.payload);
    });

    return () => {
      unlistenQueue.then((f) => f());
    };
  }, []);

  const handlePasteQueuePop = useCallback(async () => {
    try {
      await invoke("paste_queue_pop");
    } catch (err) {
      console.error("连贴回填失败:", err);
    }
  }, []);

  const handleStopPasteQueue = useCallback(async () => {
    try {
      await invoke("stop_paste_queue");
    } catch (err) {
      console.error("停止连贴队列失败:", err);
    }
  }, []);

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
          <button className="capsule-action-btn pop" onClick={handlePasteQueuePop} title="手动回填下一项">
            回填
          </button>
        )}
        <button className="capsule-action-btn stop" onClick={handleStopPasteQueue} title="退出连贴模式 (Alt+Shift+C)">
          退出
        </button>
      </div>
    </div>
  );
};

const MainPanel: React.FC = () => {
  const [activeTab, setActiveTab] = useState<ActiveTab>("history");
  const [filterCategory, setFilterCategory] = useState<FilterCategory>("all");
  const [query, setQuery] = useState<string>("");
  const [rawDisplayItems, setRawDisplayItems] = useState<DisplayItem[]>([]);
  const [selectedIndex, setSelectedIndex] = useState<number>(0);
  const [drawerOpen, setDrawerOpen] = useState<boolean>(false);
  const [temporaryRevealId, setTemporaryRevealId] = useState<number | null>(null);

  // 动作面板与图片放大预览
  const [actionPaletteOpen, setActionPaletteOpen] = useState<boolean>(false);
  const [actionSelectedIndex, setActionSelectedIndex] = useState<number>(0);
  const [transformError, setTransformError] = useState<string | null>(null);
  const [imageDetails, setImageDetails] = useState<Record<string, ImageDetail>>({});
  const [previewModalOpen, setPreviewModalOpen] = useState<boolean>(false);
  const [ocrTextMap, setOcrTextMap] = useState<Record<number, string>>({});
  const [ocrLoading, setOcrLoading] = useState<boolean>(false);

  // 多选状态
  const [selectedIds, setSelectedIds] = useState<number[]>([]);

  // 连贴队列
  const [queueStatus, setQueueStatus] = useState<QueueStatus>({
    is_active: false,
    count: 0,
    items: [],
  });

  // 短语编辑模态框
  const [snippetModalOpen, setSnippetModalOpen] = useState<boolean>(false);
  const [editingSnippet, setEditingSnippet] = useState<Snippet | null>(null);
  const [snippetTitle, setSnippetTitle] = useState<string>("");
  const [snippetShortcut, setSnippetShortcut] = useState<string>("");
  const [snippetContent, setSnippetContent] = useState<string>("");
  const [snippetFormError, setSnippetFormError] = useState<string | null>(null);

  // 隐身模式
  const [incognitoStatus, setIncognitoStatus] = useState<IncognitoStatus>({
    is_active: false,
    expires_at: null,
    remaining_seconds: null,
  });

  // 设置模态框
  const [settingsModalOpen, setSettingsModalOpen] = useState<boolean>(false);
  const [autostartEnabled, setAutostartEnabled] = useState<boolean>(false);
  const [backupMsg, setBackupMsg] = useState<{ type: "success" | "error"; text: string } | null>(null);
  const [backupLoading, setBackupLoading] = useState<boolean>(false);

  // 存储信息状态 (Issue #19)
  const [storageInfo, setStorageInfo] = useState<StorageInfo | null>(null);
  const [copyPathFeedback, setCopyPathFeedback] = useState<boolean>(false);

  // 历史容量上限治理 (Ticket #27)
  const [historyCapacity, setHistoryCapacity] = useState<number>(500);

  // 色彩数值一键复制反馈 (Ticket #26)
  const [copiedColorKey, setCopiedColorKey] = useState<string | null>(null);

  // 在线更新检查状态 (Issue #19 & #20)
  const [updateChecking, setUpdateChecking] = useState<boolean>(false);
  const [updateInfo, setUpdateInfo] = useState<GitHubReleaseInfo | null>(null);
  const [updateModalOpen, setUpdateModalOpen] = useState<boolean>(false);
  const [updateFeedback, setUpdateFeedback] = useState<{ type: "success" | "error" | "info"; text: string } | null>(null);
  const [downloadingUpdate, setDownloadingUpdate] = useState<boolean>(false);
  const [downloadProgress, setDownloadProgress] = useState<{
    received: number;
    total: number;
    percent: number;
    statusText: string;
  }>({ received: 0, total: 0, percent: 0, statusText: "" });
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const downloadAbortRef = useRef<AbortController | null>(null);

  // 是否存在可用新版本 (Update Beacon)
  const hasNewVersion = useMemo(() => {
    if (!updateInfo) return false;
    return compareSemVer(CURRENT_VERSION, updateInfo.tag_name) > 0;
  }, [updateInfo]);

  // 待提交物理删除与 3 秒撤销网 (Entry Deletion & Safety Net)
  const [pendingDelete, setPendingDelete] = useState<{ items: DisplayItem[]; timerId: ReturnType<typeof setTimeout> } | null>(null);
  const [undoToast, setUndoToast] = useState<{ text: string; showUndo: boolean } | null>(null);
  const pendingDeleteRef = useRef(pendingDelete);
  pendingDeleteRef.current = pendingDelete;

  // 窗口钉住置顶 (WindowPinning) 与失焦自动隐藏保护
  const [isPinned, setIsPinned] = useState<boolean>(() => {
    return localStorage.getItem("clip_window_pinned") === "true";
  });
  const [autoHideOnBlur, setAutoHideOnBlur] = useState<boolean>(() => {
    const saved = localStorage.getItem("clip_auto_hide_blur");
    return saved !== null ? saved === "true" : true;
  });

  // 全局唤起热键状态 (GlobalShortcutManager)
  const [globalShortcut, setGlobalShortcut] = useState<string>("Alt+V");
  const [shortcutDraft, setShortcutDraft] = useState<string>("Alt+V");
  const [isRecordingShortcut, setIsRecordingShortcut] = useState<boolean>(false);
  const [shortcutFeedback, setShortcutFeedback] = useState<{ type: "success" | "error"; text: string } | null>(null);

  const inputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const revealTimerRef = useRef<number | null>(null);

  // 状态镜像 Ref：供单例事件监听器与操作系统失焦回调无延迟同步读取，杜绝依赖项变更导致的重绑风暴
  const selectedIndexRef = useRef(selectedIndex);
  useEffect(() => {
    selectedIndexRef.current = selectedIndex;
  }, [selectedIndex]);

  const isPinnedRef = useRef(isPinned);
  useEffect(() => {
    isPinnedRef.current = isPinned;
  }, [isPinned]);

  const autoHideOnBlurRef = useRef(autoHideOnBlur);
  useEffect(() => {
    autoHideOnBlurRef.current = autoHideOnBlur;
  }, [autoHideOnBlur]);

  // 窗口拖动中状态守护（防止未钉住时拖动引发失焦闪退）
  const isDraggingRef = useRef<boolean>(false);
  const dragResetTimerRef = useRef<number | null>(null);

  const activeTabRef = useRef(activeTab);
  useEffect(() => {
    activeTabRef.current = activeTab;
  }, [activeTab]);

  const queryRef = useRef(query);
  useEffect(() => {
    queryRef.current = query;
  }, [query]);

  /**
   * 加载数据并生成统一直观展示模型
   */
  const loadData = useCallback(async (tab: ActiveTab, searchQuery: string) => {
    try {
      const q = searchQuery.trim();

      // 1. 独立短语 Tab 模式
      if (tab === "snippets") {
        let snippetsList: Snippet[];
        if (q.length === 0) {
          snippetsList = await invoke<Snippet[]>("get_snippets");
        } else {
          snippetsList = await invoke<Snippet[]>("search_snippets", { query: q });
        }
        setRawDisplayItems(
          snippetsList.map((s) => ({
            id: s.id,
            isSnippet: true,
            content: s.content,
            title: s.title,
            shortcut: s.shortcut,
            entry_type: "snippet",
            created_at: s.updated_at,
            is_pinned: false,
            kind: "snippet",
            rawSnippet: s,
          }))
        );
        setSelectedIndex(0);
        return;
      }

      // 2. 剪贴板历史 Tab 下以 / 开头触发短语快速搜索
      if (q.startsWith("/")) {
        const snippetsList = await invoke<Snippet[]>("search_snippets", { query: q });
        setRawDisplayItems(
          snippetsList.map((s) => ({
            id: s.id,
            isSnippet: true,
            content: s.content,
            title: s.title,
            shortcut: s.shortcut,
            entry_type: "snippet",
            created_at: s.updated_at,
            is_pinned: false,
            kind: "snippet",
            rawSnippet: s,
          }))
        );
        setSelectedIndex(0);
        return;
      }

      // 3. 常规剪贴板历史拉取
      let history: ClipboardEntry[];
      if (q.length === 0) {
        history = await invoke<ClipboardEntry[]>("get_history", { limit: 100 });
      } else {
        history = await invoke<ClipboardEntry[]>("search_history", { query: q, limit: 100 });
      }

      // 若有关键词搜索，同时混合检索短语并置顶高亮微标展示
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
        kind: "snippet",
        rawSnippet: s,
      }));

      const historyItems: DisplayItem[] = history.map((item) => {
        const isImage = item.entry_type === "image";
        const { kind, title } = analyzeContent(item.content, isImage, false);
        return {
          id: item.id,
          isSnippet: false,
          content: item.content,
          title,
          entry_type: item.entry_type,
          created_at: item.created_at,
          is_pinned: item.is_pinned,
          kind,
          rawEntry: item,
        };
      });

      setRawDisplayItems([...snippetItems, ...historyItems]);
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
   * 6 大分类过滤后的展示列表
   */
  const displayItems = useMemo(() => {
    if (activeTab === "snippets") {
      return rawDisplayItems;
    }
    return rawDisplayItems.filter((item) => {
      if (filterCategory === "all") return true;
      if (filterCategory === "pinned") return item.is_pinned;
      if (filterCategory === "image") return item.kind === "image";
      if (filterCategory === "code") return item.kind === "code";
      if (filterCategory === "link") return item.kind === "link";
      if (filterCategory === "text") return item.kind === "text" || item.kind === "sensitive" || item.kind === "color";
      return true;
    });
  }, [activeTab, filterCategory, rawDisplayItems]);

  const displayItemsRef = useRef(displayItems);
  useEffect(() => {
    displayItemsRef.current = displayItems;
  }, [displayItems]);

  /**
   * 触发条目极速回填
   */
  const handlePaste = useCallback(async (id: number) => {
    try {
      await invoke("paste_entry", { id });
      setPreviewModalOpen(false);
      setDrawerOpen(false);
    } catch (err) {
      console.error("回填剪贴板条目失败:", err);
    }
  }, []);

  /**
   * 触发常用短语回填
   */
  const handlePasteSnippet = useCallback(async (id: number) => {
    try {
      await invoke("paste_snippet", { id });
      setPreviewModalOpen(false);
      setDrawerOpen(false);
    } catch (err) {
      console.error("回填常用短语失败:", err);
    }
  }, []);

  /**
   * 统一粘贴分发器 (消除 Duplicated Code 坏味道)
   */
  const handleDispatchPaste = useCallback(
    (item: DisplayItem) => {
      if (item.isSnippet) {
        handlePasteSnippet(item.id);
      } else {
        handlePaste(item.id);
      }
    },
    [handlePaste, handlePasteSnippet]
  );

  /**
   * 切换单个条目的多选勾选状态
   */
  const handleToggleSelectItem = useCallback((id: number, e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    setSelectedIds((prev) =>
      prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id]
    );
  }, []);

  /**
   * 范围连选条目 (Shift + 点击)
   */
  const handleRangeSelectItem = useCallback((targetIndex: number, e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    const start = Math.min(selectedIndex, targetIndex);
    const end = Math.max(selectedIndex, targetIndex);
    const rangeIds = displayItems.slice(start, end + 1).map((item) => item.id);
    setSelectedIds((prev) => {
      const set = new Set(prev);
      rangeIds.forEach((id) => set.add(id));
      return Array.from(set);
    });
  }, [selectedIndex, displayItems]);

  /**
   * 按住空白区域平滑拖动无边框窗口 (仅在固定状态生效，非固定状态不触发任何拖拽)
   */
  const handleStartDrag = useCallback((e: React.MouseEvent) => {
    if (!isPinnedRef.current) return;
    if (e.button === 0) {
      const target = e.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "BUTTON" ||
        target.tagName === "A" ||
        target.closest("button") ||
        target.closest("input") ||
        target.closest(".search-wrapper") ||
        target.closest(".item-card") ||
        target.closest(".panel-item") ||
        target.closest(".segmented-pill") ||
        target.closest(".modal-card") ||
        target.closest(".multi-select-toolbar")
      ) {
        return;
      }
      if (dragResetTimerRef.current !== null) {
        window.clearTimeout(dragResetTimerRef.current);
        dragResetTimerRef.current = null;
      }
      isDraggingRef.current = true;
      getCurrentWebviewWindow().startDragging();
    }
  }, []);

  // 释放鼠标拖拽后延迟重置拖动标记，彻底抵御 Windows 释放焦点微抖动
  useEffect(() => {
    const handleGlobalMouseUp = () => {
      if (isDraggingRef.current) {
        dragResetTimerRef.current = window.setTimeout(() => {
          isDraggingRef.current = false;
        }, 300);
      }
    };
    window.addEventListener("mouseup", handleGlobalMouseUp);
    return () => {
      window.removeEventListener("mouseup", handleGlobalMouseUp);
      if (dragResetTimerRef.current !== null) {
        window.clearTimeout(dragResetTimerRef.current);
      }
    };
  }, []);

  /**
   * 多选条目换行合并回填
   */
  const handlePasteMultiple = useCallback(async (ids: number[]) => {
    if (ids.length === 0) return;
    try {
      await invoke("paste_multiple_entries", { ids, separator: "\n" });
      setSelectedIds([]);
      setPreviewModalOpen(false);
      setDrawerOpen(false);
    } catch (err) {
      console.error("多选合并回填失败:", err);
    }
  }, []);

  /**
   * 连贴收集与队列控制
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
   * 切换隐身模式
   */
  const handleToggleIncognito = useCallback(async (durationMinutes?: number) => {
    try {
      const status = await invoke<IncognitoStatus>("toggle_incognito", {
        durationMinutes: durationMinutes ?? null,
      });
      setIncognitoStatus(status);
    } catch (err) {
      console.error("切换隐身模式失败:", err);
    }
  }, []);

  /**
   * 开机自启动设置
   */
  const handleToggleAutostart = useCallback(async () => {
    try {
      const res = await invoke<boolean>("set_autostart", { enable: !autostartEnabled });
      setAutostartEnabled(res);
    } catch (err) {
      console.error("设置开机自启失败:", err);
    }
  }, [autostartEnabled]);

  /**
   * 切换窗口钉住置顶状态 (WindowPinning)
   */
  const handleToggleWindowPin = useCallback(() => {
    setIsPinned((prev) => {
      const next = !prev;
      localStorage.setItem("clip_window_pinned", String(next));
      return next;
    });
  }, []);

  /**
   * 切换失焦自动隐藏行为
   */
  const handleToggleAutoHideOnBlur = useCallback(() => {
    setAutoHideOnBlur((prev) => {
      const next = !prev;
      localStorage.setItem("clip_auto_hide_blur", String(next));
      return next;
    });
  }, []);

  /**
   * 保存并动态注册新的全局唤起快捷键 (GlobalShortcutManager)
   */
  const handleSaveShortcut = useCallback(
    async (targetShortcut: string) => {
      try {
        setShortcutFeedback(null);
        const updated = await invoke<string>("set_global_shortcut", {
          shortcut: targetShortcut,
        });
        setGlobalShortcut(updated);
        setShortcutDraft(updated);
        setIsRecordingShortcut(false);
        setShortcutFeedback({
          type: "success",
          text: `全局快捷键已成功更新为：${updated}`,
        });
      } catch (err: any) {
        const errMsg = typeof err === "string" ? err : err?.message || "快捷键设置失败";
        setShortcutFeedback({ type: "error", text: errMsg });
      }
    },
    []
  );

  /**
   * 一键恢复默认全局快捷键 (Alt+V)
   */
  const handleResetDefaultShortcut = useCallback(async () => {
    await handleSaveShortcut("Alt+V");
  }, [handleSaveShortcut]);

  /**
   * 快捷键录制器键盘事件捕获
   */
  const handleShortcutKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      setIsRecordingShortcut(false);
      setShortcutDraft(globalShortcut);
      return;
    }

    const modifiers: string[] = [];
    if (e.ctrlKey) modifiers.push("Ctrl");
    if (e.altKey) modifiers.push("Alt");
    if (e.shiftKey) modifiers.push("Shift");
    if (e.metaKey) modifiers.push("Super");

    const key = e.key;
    if (["Control", "Alt", "Shift", "Meta"].includes(key)) {
      return;
    }

    let keyName = key.toUpperCase();
    if (key === " ") keyName = "Space";
    if (key.length === 1) keyName = key.toUpperCase();

    if (modifiers.length === 0) {
      setShortcutFeedback({
        type: "error",
        text: "必须包含至少一个修饰键 (Ctrl、Alt、Shift 或 Win)",
      });
      return;
    }

    const combined = [...modifiers, keyName].join("+");
    setShortcutDraft(combined);
    setShortcutFeedback(null);
  };

  /**
   * 刷新存储信息
   */
  const loadStorageInfo = useCallback(async () => {
    try {
      const info = await invoke<StorageInfo>("get_storage_info");
      setStorageInfo(info);
    } catch (err) {
      console.error("获取存储信息失败:", err);
    }
  }, []);

  /**
   * 刷新历史容量上限 (Ticket #27)
   */
  const loadHistoryCapacity = useCallback(async () => {
    try {
      const cap = await invoke<number>("get_history_capacity");
      if (cap) {
        setHistoryCapacity(cap);
      }
    } catch (err) {
      console.error("获取历史容量上限失败:", err);
    }
  }, []);

  /**
   * 变更历史容量上限并触发 LRU 裁剪与孤立 Blob 清理 (Ticket #27)
   */
  const handleSetCapacity = useCallback(
    async (capacity: number) => {
      try {
        await invoke("set_history_capacity", { capacity });
        setHistoryCapacity(capacity);
        await loadData(activeTab, query);
        await loadStorageInfo();
      } catch (err) {
        console.error("更新历史容量上限失败:", err);
      }
    },
    [loadData, activeTab, query, loadStorageInfo]
  );

  /**
   * 一键复制色彩格式值并展示临时对勾反馈 (Ticket #26)
   */
  const handleCopyColorValue = useCallback(async (colorVal: string) => {
    try {
      await navigator.clipboard.writeText(colorVal);
      setCopiedColorKey(colorVal);
      setTimeout(() => {
        setCopiedColorKey((prev) => (prev === colorVal ? null : prev));
      }, 1500);
    } catch (err) {
      console.error("复制色彩值失败:", err);
    }
  }, []);

  useEffect(() => {
    loadHistoryCapacity();
  }, [loadHistoryCapacity]);

  useEffect(() => {
    if (settingsModalOpen) {
      loadStorageInfo();
      loadHistoryCapacity();
    }
  }, [settingsModalOpen, loadStorageInfo, loadHistoryCapacity]);

  /**
   * 在操作系统文件资源管理器中打开存储目录
   */
  const handleOpenStorageDir = useCallback(async () => {
    try {
      await invoke("open_storage_dir");
    } catch (err) {
      console.error("打开存储目录失败:", err);
    }
  }, []);

  /**
   * 复制存储路径至剪贴板
   */
  const handleCopyStoragePath = useCallback((path: string) => {
    if (!path) return;
    navigator.clipboard.writeText(path);
    setCopyPathFeedback(true);
    setTimeout(() => setCopyPathFeedback(false), 2000);
  }, []);

  /**
   * 检查在线 GitHub 发布版本（基于后端双通道免配额架构，免疫 403 Rate Limit）
   */
  const handleCheckUpdate = useCallback(async (isManual = true) => {
    setUpdateChecking(true);
    setUpdateFeedback(null);
    try {
      const data = await invoke<GitHubReleaseInfo>("check_for_updates");
      const hasNew = compareSemVer(CURRENT_VERSION, data.tag_name) > 0;
      if (hasNew) {
        setUpdateInfo(data);
        setDownloadingUpdate(false);
        setDownloadError(null);
        if (isManual) {
          setUpdateModalOpen(true);
        }
      } else {
        if (isManual) {
          setUpdateFeedback({
            type: "success",
            text: `当前已是最新版本 (${CURRENT_VERSION})`,
          });
        }
      }
    } catch (err: any) {
      if (isManual) {
        setUpdateFeedback({
          type: "error",
          text: `${typeof err === "string" ? err : err?.message || err}`,
        });
      }
    } finally {
      setUpdateChecking(false);
    }
  }, []);

  /**
   * 取消当前进行中的更新下载 (Issue #20)
   */
  const handleCancelUpdateDownload = useCallback(() => {
    if (downloadAbortRef.current) {
      downloadAbortRef.current.abort();
      downloadAbortRef.current = null;
    }
    setDownloadingUpdate(false);
    setDownloadProgress({ received: 0, total: 0, percent: 0, statusText: "" });
  }, []);

  // 监听 Rust 原生下载引擎流式进度事件 (update-download-progress)
  useEffect(() => {
    const unlistenPromise = listen<{
      received_bytes: number;
      total_bytes: number;
      percentage: number;
      status: string;
    }>("update-download-progress", (event) => {
      setDownloadProgress({
        received: event.payload.received_bytes,
        total: event.payload.total_bytes,
        percent: Math.round(event.payload.percentage),
        statusText: event.payload.status,
      });
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  /**
   * 一键流式下载并静默覆盖升级 (基于 Rust 原生下载引擎，免疫 CORS 跨域限制，自动继承系统代理)
   */
  const handleStartInPlaceUpdate = useCallback(async () => {
    if (!updateInfo) return;

    const assets = updateInfo.assets || [];
    // 1. 优先匹配 Windows x64 安装包
    const setupAsset =
      assets.find((a) => a.name.includes("Windows") && a.name.endsWith(".exe")) ||
      assets.find((a) => a.name.endsWith("-Setup.exe") || a.name.endsWith(".exe"));

    if (!setupAsset) {
      setDownloadError("未检测到当前平台的自动化安装包，请点击下方前往 GitHub 网页下载。");
      return;
    }

    const checksumsAsset = assets.find((a) => a.name === "checksums.txt");
    let targetSha256: string | null = null;

    setDownloadingUpdate(true);
    setDownloadError(null);
    setDownloadProgress({
      received: 0,
      total: setupAsset.size || 0,
      percent: 0,
      statusText: "准备连接原生下载引擎...",
    });

    try {
      // 2. 尝试读取 checksums.txt 清单以执行 Checksum Gate 强校验
      if (checksumsAsset) {
        setDownloadProgress((prev) => ({ ...prev, statusText: "正在读取校验清单..." }));
        const checkCandidates = [
          checksumsAsset.browser_download_url,
          `https://gh-proxy.com/${checksumsAsset.browser_download_url}`,
          `https://ghproxy.net/${checksumsAsset.browser_download_url}`,
        ];
        for (const checkUrl of checkCandidates) {
          try {
            const checkResp = await fetch(checkUrl);
            if (checkResp.ok) {
              const checkText = await checkResp.text();
              for (const line of checkText.split("\n")) {
                const parts = line.trim().split(/\s+/);
                if (parts.length >= 2 && parts[1] === setupAsset.name) {
                  targetSha256 = parts[0];
                  break;
                }
              }
              if (targetSha256) break;
            }
          } catch {
            // 节点容灾回退
          }
        }
      }

      // 3. 触发 Rust 原生流式下载与静默安装
      await invoke("download_and_install_update", {
        url: setupAsset.browser_download_url,
        fileName: setupAsset.name,
        expectedSha256: targetSha256,
      });

      setDownloadProgress((prev) => ({
        ...prev,
        statusText: "✓ 升级安装器已启动，正在原地覆盖更新并重启...",
      }));
    } catch (err: any) {
      setDownloadError(`更新失败: ${typeof err === "string" ? err : err?.message || err}`);
      setDownloadingUpdate(false);
    }
  }, [updateInfo]);

  /**
   * 唤起系统原生另存为对话框并执行备份导出 (Issue #19)
   */
  const handleExportBackup = useCallback(async () => {
    setBackupMsg(null);
    try {
      const targetPath = await invoke<string | null>("select_backup_save_path");
      if (!targetPath) {
        return; // 用户取消另存为
      }
      setBackupLoading(true);
      const manifest = await invoke<BackupManifest>("export_backup", { path: targetPath });
      setBackupMsg({
        type: "success",
        text: `✓ 备份导出成功！已归档 ${manifest.entry_count} 条历史、${manifest.snippet_count} 条常用短语及 ${manifest.blob_count} 个图片。`,
      });
      loadStorageInfo();
    } catch (err) {
      setBackupMsg({
        type: "error",
        text: `导出失败: ${err}`,
      });
    } finally {
      setBackupLoading(false);
    }
  }, [loadStorageInfo]);

  /**
   * 唤起系统原生文件选择器并执行备份恢复 (Issue #19)
   */
  const handleImportBackup = useCallback(async () => {
    setBackupMsg(null);
    try {
      const sourcePath = await invoke<string | null>("select_backup_open_path");
      if (!sourcePath) {
        return; // 用户取消选择
      }
      if (!window.confirm(`确定要从以下归档文件恢复数据吗？\n\n${sourcePath}\n\n注意：此操作将合并导入归档中的历史记录与常用短语。`)) {
        return;
      }
      setBackupLoading(true);
      const manifest = await invoke<BackupManifest>("import_backup", { path: sourcePath });
      setBackupMsg({
        type: "success",
        text: `✓ 恢复成功！已还原 ${manifest.entry_count} 条历史与 ${manifest.snippet_count} 条短语。`,
      });
      loadData(activeTab, query);
      loadStorageInfo();
    } catch (err) {
      setBackupMsg({
        type: "error",
        text: `导入还原失败: ${err}`,
      });
    } finally {
      setBackupLoading(false);
    }
  }, [loadData, activeTab, query, loadStorageInfo]);

  const handleClearAllHistory = useCallback(async () => {
    if (!window.confirm("确定要清空全部剪贴板历史记录和图片缓存吗？（常用短语将保留）")) {
      return;
    }
    try {
      await invoke("clear_all_history");
      loadData(activeTab, query);
      setBackupMsg({
        type: "success",
        text: "已成功清空全部剪贴板历史记录。",
      });
    } catch (err) {
      setBackupMsg({
        type: "error",
        text: `清空失败: ${err}`,
      });
    }
  }, [loadData, activeTab, query]);

  /**
   * 原生离线 OCR
   */
  const handleOcr = useCallback(
    async (id: number) => {
      setOcrLoading(true);
      setTransformError(null);
      try {
        const recognized = await invoke<string>("ocr_image_entry", { id });
        setOcrTextMap((prev) => ({ ...prev, [id]: recognized }));
        setDrawerOpen(true);
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
   * 动作格式转换并回填
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
   * 纯文本无格式强制回填 (Shift + Enter)
   */
  const handlePlainPaste = useCallback(async (id: number) => {
    try {
      await invoke("paste_plain_entry", { id });
    } catch (err: unknown) {
      console.error("纯文本回填失败:", err);
    }
  }, []);

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

  const handleTogglePin = useCallback(
    async (id: number, e?: React.MouseEvent) => {
      if (e) e.stopPropagation();
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
   * 常用短语弹窗控制
   */
  const handleOpenCreateSnippet = useCallback(() => {
    setEditingSnippet(null);
    setSnippetTitle("");
    setSnippetShortcut("");
    setSnippetContent("");
    setSnippetFormError(null);
    setSnippetModalOpen(true);
  }, []);

  const handleOpenEditSnippet = useCallback((snippet: Snippet, e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    setEditingSnippet(snippet);
    setSnippetTitle(snippet.title);
    setSnippetShortcut(snippet.shortcut);
    setSnippetContent(snippet.content);
    setSnippetFormError(null);
    setSnippetModalOpen(true);
  }, []);

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
   * 敏感凭据 3 秒临时显露明文 (带定时器安全管理与行内直达)
   */
  const handleRevealSensitive = useCallback((id: number) => {
    if (revealTimerRef.current !== null) {
      window.clearTimeout(revealTimerRef.current);
    }
    setTemporaryRevealId(id);
    revealTimerRef.current = window.setTimeout(() => {
      setTemporaryRevealId((prev) => (prev === id ? null : prev));
      revealTimerRef.current = null;
    }, SENSITIVE_REVEAL_TIMEOUT_MS);
  }, []);

  // 卸载时清理定时器，杜绝异步内存泄漏
  useEffect(() => {
    return () => {
      if (revealTimerRef.current !== null) {
        window.clearTimeout(revealTimerRef.current);
      }
    };
  }, []);

  // 键盘漫游时确保高亮项目处于可见视口 (Issue #15)
  useEffect(() => {
    if (!listRef.current) return;
    const items = listRef.current.children;
    if (selectedIndex >= 0 && selectedIndex < items.length) {
      (items[selectedIndex] as HTMLElement)?.scrollIntoView({ block: "nearest" });
    }
  }, [selectedIndex]);

  /**
   * 浏览器外链打开
   */
  const handleOpenLinkInBrowser = useCallback((url: string) => {
    openUrl(url).catch((err) => console.error("在浏览器打开链接失败:", err));
  }, []);

  /**
   * 提交物理删除条目至 SQLite 及文件系统
   */
  const commitPendingDelete = useCallback(async (itemsToDelete: DisplayItem[]) => {
    try {
      const historyIds = itemsToDelete.filter((i) => !i.isSnippet).map((i) => i.id);
      const snippetIds = itemsToDelete.filter((i) => i.isSnippet).map((i) => i.id);
      if (historyIds.length === 1) {
        await invoke("delete_entry", { id: historyIds[0] });
      } else if (historyIds.length > 1) {
        await invoke("delete_entries", { ids: historyIds });
      }
      for (const sid of snippetIds) {
        await invoke("delete_snippet", { id: sid });
      }
    } catch (err) {
      console.error("提交物理删除条目失败:", err);
    }
  }, []);

  /**
   * 触发条目删除（乐观更新与 3 秒撤销网）
   */
  const handleDeleteItems = useCallback((itemsToDelete: DisplayItem[]) => {
    if (itemsToDelete.length === 0) return;
    if (pendingDeleteRef.current) {
      clearTimeout(pendingDeleteRef.current.timerId);
      commitPendingDelete(pendingDeleteRef.current.items);
    }
    const deleteIds = new Set(itemsToDelete.map((i) => `${i.isSnippet ? "s" : "e"}-${i.id}`));
    setRawDisplayItems((prev) => prev.filter((i) => !deleteIds.has(`${i.isSnippet ? "s" : "e"}-${i.id}`)));
    setSelectedIds([]);
    setSelectedIndex(0);

    const timerId = setTimeout(() => {
      commitPendingDelete(itemsToDelete);
      setPendingDelete(null);
      setUndoToast(null);
    }, 3000);

    setPendingDelete({ items: itemsToDelete, timerId });
    setUndoToast({
      text: `已删除 ${itemsToDelete.length} 项`,
      showUndo: true,
    });
  }, [commitPendingDelete]);

  /**
   * 撤销最近一次删除 (Ctrl+Z)
   */
  const handleUndoDelete = useCallback(() => {
    if (!pendingDeleteRef.current) return;
    clearTimeout(pendingDeleteRef.current.timerId);
    const restoredCount = pendingDeleteRef.current.items.length;
    setPendingDelete(null);
    loadData(activeTabRef.current, queryRef.current);
    setUndoToast({
      text: `✓ 已撤销恢复 ${restoredCount} 项记录`,
      showUndo: false,
    });
    setTimeout(() => setUndoToast(null), 2500);
  }, [loadData]);

  /**
   * 主动隐藏悬浮面板
   */
  const handleClose = useCallback(async () => {
    if (pendingDeleteRef.current) {
      clearTimeout(pendingDeleteRef.current.timerId);
      commitPendingDelete(pendingDeleteRef.current.items);
      setPendingDelete(null);
      setUndoToast(null);
    }
    setActionPaletteOpen(false);
    setPreviewModalOpen(false);
    setSnippetModalOpen(false);
    setSettingsModalOpen(false);
    setDrawerOpen(false);
    try {
      await invoke("hide_window");
    } catch (err) {
      console.error("隐藏窗口失败:", err);
    }
  }, [commitPendingDelete]);

  const handleQueryChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.value;
    setQuery(val);
    loadData(activeTab, val);
  };

  const handleTabChange = (newTab: ActiveTab) => {
    setActiveTab(newTab);
    loadData(newTab, query);
    inputRef.current?.focus();
  };

  // 监听后端广播事件与原生窗口级焦点生命周期 (初始化单次注册，杜绝依赖项变更导致的频繁解绑与闪烁风暴)
  useEffect(() => {
    loadData("history", "");

    invoke<QueueStatus>("get_paste_queue_status")
      .then(setQueueStatus)
      .catch((err) => console.error("获取连贴状态失败:", err));

    invoke<IncognitoStatus>("get_incognito_status")
      .then(setIncognitoStatus)
      .catch((err) => console.error("获取隐身状态失败:", err));

    invoke<boolean>("is_autostart_enabled")
      .then(setAutostartEnabled)
      .catch((err) => console.error("获取自启配置失败:", err));

    const unlistenClipboard = listen<ClipboardEntry>("clipboard-changed", () => {
      loadData(activeTabRef.current, queryRef.current);
    });

    const unlistenQueue = listen<QueueStatus>("paste-queue-changed", (event) => {
      setQueueStatus(event.payload);
    });

    const unlistenIncognito = listen<IncognitoStatus>("incognito-changed", (event) => {
      setIncognitoStatus(event.payload);
    });

    const unlistenRestored = listen("data-restored", () => {
      loadData(activeTabRef.current, queryRef.current);
    });

    const unlistenPanelShown = listen("panel-shown", () => {
      setQuery("");
      setActiveTab("history");
      setFilterCategory("all");
      loadData("history", "");
      setSelectedIndex(0);
      setSelectedIds([]);
      setDrawerOpen(false);
      setActionPaletteOpen(false);
      setPreviewModalOpen(false);
      setSnippetModalOpen(false);
      setSettingsModalOpen(false);

      setTimeout(() => {
        inputRef.current?.blur();
      }, 20);
    });

    invoke<string>("get_global_shortcut")
      .then((sc) => {
        setGlobalShortcut(sc);
        setShortcutDraft(sc);
      })
      .catch((err) => console.error("获取全局快捷键失败:", err));

    const unlistenShortcutChanged = listen<string>("global-shortcut-changed", (event) => {
      setGlobalShortcut(event.payload);
      setShortcutDraft(event.payload);
    });

    // 原生操作系统级窗口焦点监听：仅当系统窗口真正失去焦点 (点击外部程序/桌面) 时判定，彻底杜绝 DOM 内部切换按钮与输入框导致的误关
    const appWin = getCurrentWebviewWindow();
    const unlistenFocus = appWin.onFocusChanged(({ payload: focused }) => {
      if (!focused) {
        if (isPinnedRef.current || !autoHideOnBlurRef.current || isDraggingRef.current) {
          return;
        }
        handleClose();
      }
    });

    // 启动延迟 3 秒发起后台静默检查更新 (Update Beacon)
    const updateTimer = setTimeout(() => {
      handleCheckUpdate(false);
    }, 3000);

    return () => {
      clearTimeout(updateTimer);
      unlistenClipboard.then((f) => f());
      unlistenQueue.then((f) => f());
      unlistenIncognito.then((f) => f());
      unlistenRestored.then((f) => f());
      unlistenPanelShown.then((f) => f());
      unlistenShortcutChanged.then((f) => f());
      unlistenFocus.then((f) => f());
    };
  }, [loadData, handleClose, handleCheckUpdate]);

  // 全局键盘导航流闭环 (Issue #15)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // 1. 输入法合成保护
      if (e.isComposing) {
        return;
      }

      // 2. 逐级退出 (Hierarchical Dismissal: Escape)
      if (e.key === "Escape") {
        e.preventDefault();
        if (previewModalOpen) {
          setPreviewModalOpen(false);
          return;
        }
        if (actionPaletteOpen) {
          setActionPaletteOpen(false);
          return;
        }
        if (drawerOpen) {
          setDrawerOpen(false);
          return;
        }
        if (settingsModalOpen) {
          setSettingsModalOpen(false);
          return;
        }
        if (snippetModalOpen) {
          setSnippetModalOpen(false);
          return;
        }
        if (selectedIds.length > 0) {
          setSelectedIds([]);
          return;
        }
        if (document.activeElement === inputRef.current) {
          inputRef.current?.blur();
          return;
        }
        handleClose();
        return;
      }

      // 3. 模态框激活时的键盘路由
      if (settingsModalOpen || snippetModalOpen) {
        return;
      }

      // 4. Ctrl + Z: 撤销最近一次删除 (Safety Net)
      if (e.ctrlKey && (e.key === "z" || e.key === "Z")) {
        if (pendingDeleteRef.current) {
          e.preventDefault();
          handleUndoDelete();
          return;
        }
      }

      // 5. Delete 键: 单选/多选删除当前项 (Entry Deletion)
      if (e.key === "Delete") {
        const isInputFocused = document.activeElement === inputRef.current;
        if (!isInputFocused) {
          if (selectedIds.length > 0) {
            e.preventDefault();
            const targets = displayItems.filter((i) => selectedIds.includes(i.id));
            handleDeleteItems(targets);
            return;
          }
          const current = displayItems[selectedIndex];
          if (current) {
            e.preventDefault();
            handleDeleteItems([current]);
            return;
          }
        }
      }

      if (previewModalOpen) {
        if (e.key === " ") {
          e.preventDefault();
          setPreviewModalOpen(false);
          return;
        }
        if (e.key === "Enter") {
          e.preventDefault();
          const current = displayItems[selectedIndex];
          if (current) handleDispatchPaste(current);
          return;
        }
        return;
      }

      // 4. Action Palette 处于激活态时的键盘路由
      if (actionPaletteOpen) {
        const current = displayItems[selectedIndex];
        const activeActions = current?.kind === "image" ? IMAGE_ACTIONS : TRANSFORM_ACTIONS;

        if (e.key === "Tab" || (e.ctrlKey && (e.key === "k" || e.key === "K"))) {
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

        if (e.key >= "1" && e.key <= String(activeActions.length)) {
          const actionIdx = parseInt(e.key, 10) - 1;
          const action = activeActions[actionIdx];
          if (current && current.rawEntry && action) {
            e.preventDefault();
            executeAction(current.rawEntry, action.key);
            return;
          }
        }
        return;
      }

      // 5. 顶层全局快捷键
      // Ctrl + Tab: 切换 Tab 模式 (历史记录 ↔ 常用短语)
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        handleTabChange(activeTab === "history" ? "snippets" : "history");
        return;
      }

      // Ctrl + N: 新建常用短语
      if (e.ctrlKey && (e.key === "n" || e.key === "N")) {
        e.preventDefault();
        handleOpenCreateSnippet();
        return;
      }

      // Tab 或 Ctrl+K: 唤出 Action Palette 动作浮层
      if ((e.ctrlKey && (e.key === "k" || e.key === "K")) || e.key === "Tab") {
        const current = displayItems[selectedIndex];
        if (current && !current.isSnippet) {
          e.preventDefault();
          setActionSelectedIndex(0);
          setActionPaletteOpen(true);
          return;
        }
      }

      // Alt + P: 钉住/取消钉住窗口 (WindowPinning)
      if (e.altKey && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        handleToggleWindowPin();
        return;
      }

      // Ctrl + P: 置顶/取消置顶当前选中历史条目
      if (e.ctrlKey && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        const current = displayItems[selectedIndex];
        if (current && !current.isSnippet) {
          handleTogglePin(current.id);
        }
        return;
      }

      // Space 键: 仅在非输入框打字状态下展开/收起右侧抽屉，杜绝误拦截首位空格
      const isInputFocused = document.activeElement === inputRef.current;
      const isSpaceTrigger = (e.key === " " && !isInputFocused) || (e.altKey && e.key === " ");
      if (isSpaceTrigger) {
        e.preventDefault();
        setDrawerOpen((prev) => !prev);
        return;
      }

      // 提取 1~9 物理数字键 (支持主键盘 Digit1~9 与小键盘 Numpad1~9，并兼容 key 兜底)
      let digitIndex: number | null = null;
      if (e.code && e.code.startsWith("Digit")) {
        const d = parseInt(e.code.replace("Digit", ""), 10);
        if (d >= 1 && d <= 9) digitIndex = d;
      } else if (e.code && e.code.startsWith("Numpad")) {
        const d = parseInt(e.code.replace("Numpad", ""), 10);
        if (d >= 1 && d <= 9) digitIndex = d;
      } else if (e.key >= "1" && e.key <= "9") {
        digitIndex = parseInt(e.key, 10);
      }

      // Ctrl + 1~9: 全局穿透极速回填 (无论是否聚焦输入框，主键盘/小键盘均可直接穿透直贴)
      if (digitIndex !== null && e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
        const targetItem = displayItems[digitIndex - 1];
        if (targetItem) {
          e.preventDefault();
          handleDispatchPaste(targetItem);
          return;
        }
      }

      // 列表导航态 (非搜索框聚焦态) 专属键盘交互
      if (!isInputFocused) {
        // 单按 '/' 键: 激活搜索框
        if (e.key === "/" && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
          e.preventDefault();
          inputRef.current?.focus();
          return;
        }

        // 单按数字 1~9: 列表导航态下一键盲贴前序条目 (无需任何修饰键，极致秒贴)
        if (digitIndex !== null && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
          const targetItem = displayItems[digitIndex - 1];
          if (targetItem) {
            e.preventDefault();
            handleDispatchPaste(targetItem);
            return;
          }
        }
      }

      // Shift + 上下键扩展多选范围
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

      // 上下方向键无缝漫游
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((prev) => (prev < displayItems.length - 1 ? prev + 1 : prev));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((prev) => (prev > 0 ? prev - 1 : 0));
      } else if (e.key === "Enter") {
        // Shift + Enter: 纯文本强制无格式粘贴
        if (e.shiftKey) {
          e.preventDefault();
          const current = displayItems[selectedIndex];
          if (current && !current.isSnippet && current.kind !== "image") {
            handlePlainPaste(current.id);
            return;
          }
        }

        // 多选模式下按回车合并粘贴
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
        // 单项回车极速回填
        e.preventDefault();
        const current = displayItems[selectedIndex];
        if (current) {
          handleDispatchPaste(current);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    displayItems,
    selectedIndex,
    selectedIds,
    drawerOpen,
    snippetModalOpen,
    actionPaletteOpen,
    actionSelectedIndex,
    previewModalOpen,
    settingsModalOpen,
    handleClose,
    handlePaste,
    handlePasteMultiple,
    handleDispatchPaste,
    handleTransformAndPaste,
    handleTogglePin,
    handleToggleWindowPin,
    handleOpenCreateSnippet,
    handleTabChange,
    executeAction,
    handlePlainPaste,
    handleDeleteItems,
    handleUndoDelete,
    query,
  ]);

  // 当前选中的项
  const selectedItem = displayItems[selectedIndex];

  return (
    <div className={`panel-container ${isPinned ? "is-pinned" : ""}`}>
      {/* ========================================================================= */}
      {/* 二层紧凑高信息密度头部 (Two-Layer Compact Header - 高度 ≤ 82px)               */}
      {/* ========================================================================= */}
      <header className="panel-header-compact" onMouseDown={handleStartDrag}>
        {/* Row 1: 整合搜索栏、模式切换胶囊与工具入口 (44px) */}
        <div className="header-row-1">
          <div className="search-wrapper" data-tauri-drag-region="false">
            <span className="search-icon">🔍</span>
            <input
              ref={inputRef}
              type="text"
              className="search-input"
              value={query}
              onChange={handleQueryChange}
              placeholder={
                activeTab === "snippets"
                  ? "搜索常用短语 (/ 聚焦)..."
                  : "搜索历史 (/ 聚焦，1~9 直贴)..."
              }
            />
            {query && (
              <button
                className="search-clear-btn"
                onClick={() => {
                  setQuery("");
                  loadData(activeTab, "");
                  inputRef.current?.focus();
                }}
                title="清空搜索"
              >
                ✕
              </button>
            )}
          </div>

          <div className="header-controls" data-tauri-drag-region="false">
            {/* 分段模式胶囊 */}
            <div className="segmented-pill">
              <button
                className={`segmented-btn ${activeTab === "history" ? "active" : ""}`}
                onClick={() => handleTabChange("history")}
                title={`剪贴板历史模式 (${globalShortcut})`}
              >
                <span>📋 历史</span>
                <span className="pill-shortcut">{globalShortcut}</span>
              </button>
              <button
                className={`segmented-btn ${activeTab === "snippets" ? "active" : ""}`}
                onClick={() => handleTabChange("snippets")}
                title="常用短语模板库 (/)"
              >
                <span>⚡ 短语</span>
                <span className="pill-shortcut">/</span>
              </button>
            </div>

            {/* 即时抽屉检查器切换按钮 */}
            <button
              className={`header-action-btn ${drawerOpen ? "active" : ""}`}
              onClick={() => setDrawerOpen((prev) => !prev)}
              title="按空格键切换抽屉预览 (Space)"
            >
              <span>👁️</span>
              <span>{drawerOpen ? "收起" : "预览"}</span>
            </button>

            {/* 新建短语按钮 (短语模式专属) */}
            {activeTab === "snippets" && (
              <button className="header-action-btn" onClick={handleOpenCreateSnippet} title="新建短语模板 (Ctrl+N)">
                <span>+ 新建</span>
              </button>
            )}

            {/* 📌 钉住/固定窗口按钮 (WindowPinning - Alt+P) */}
            <button
              className={`header-action-btn pin-btn ${isPinned ? "active pinned" : ""}`}
              onClick={handleToggleWindowPin}
              title={isPinned ? "已固定窗口：点击外部程序不隐藏 (Alt+P)" : "固定窗口：保持悬浮不隐藏 (Alt+P)"}
            >
              <span>📌</span>
              <span>{isPinned ? "已固定" : "固定"}</span>
            </button>

            {/* 设置按钮 (带版本更新指示红点) */}
            <button
              className={`header-action-btn settings-btn ${hasNewVersion ? "has-update" : ""}`}
              onClick={() => {
                setSettingsModalOpen(true);
                setDrawerOpen(false);
                setBackupMsg(null);
                setShortcutFeedback(null);
              }}
              title={hasNewVersion ? `发现新版本 ${updateInfo?.tag_name} (点击查看)` : "系统设置与快捷键管理"}
            >
              <span>⚙️</span>
              {hasNewVersion && <span className="update-dot" />}
            </button>
          </div>
        </div>

        {/* Row 2: 6 流线型分类过滤栏与简洁统计提示 (34px) */}
        <div className="header-row-2" data-tauri-drag-region="false">
          {activeTab === "history" ? (
            <div className="filter-chips">
              <button
                className={`filter-chip ${filterCategory === "all" ? "active" : ""}`}
                onClick={() => setFilterCategory("all")}
              >
                <span>全部 ({rawDisplayItems.length})</span>
              </button>
              <button
                className={`filter-chip chip-pinned ${filterCategory === "pinned" ? "active" : ""}`}
                onClick={() => setFilterCategory("pinned")}
              >
                <span>★ 置顶</span>
              </button>
              <button
                className={`filter-chip ${filterCategory === "text" ? "active" : ""}`}
                onClick={() => setFilterCategory("text")}
              >
                <span>📄 文本</span>
              </button>
              <button
                className={`filter-chip ${filterCategory === "image" ? "active" : ""}`}
                onClick={() => setFilterCategory("image")}
              >
                <span>🖼️ 图片</span>
              </button>
              <button
                className={`filter-chip ${filterCategory === "code" ? "active" : ""}`}
                onClick={() => setFilterCategory("code")}
              >
                <span>💻 代码</span>
              </button>
              <button
                className={`filter-chip ${filterCategory === "link" ? "active" : ""}`}
                onClick={() => setFilterCategory("link")}
              >
                <span>🔗 链接</span>
              </button>
            </div>
          ) : (
            <div className="filter-chips">
              <span className="filter-chip active">常用短语模板库 ({rawDisplayItems.length})</span>
            </div>
          )}

          <div className="header-stats-label">
            {hasNewVersion ? (
              <button
                className="update-beacon-pill"
                onClick={() => setUpdateModalOpen(true)}
                title="发现新版本，点击查看更新日志并一键覆盖安装"
              >
                <span className="beacon-icon">🚀</span>
                <span className="beacon-text">新版 {updateInfo?.tag_name}</span>
              </button>
            ) : (
              <>
                <span>共 {displayItems.length} 条</span>
                <span className="stats-divider">•</span>
                <span>按 1~9 快捷直贴</span>
              </>
            )}
          </div>
        </div>
      </header>

      {/* ========================================================================= */}
      {/* 工作区 (Workspace: 严格 28×28 槽位列表 + 即时抽屉检查器)                     */}
      {/* ========================================================================= */}
      <div className="panel-workspace">
        <div className="panel-list" ref={listRef}>
          {displayItems.length === 0 ? (
            <div className="empty-state">
              <p className="empty-title">
                {query ? "未找到匹配条目" : activeTab === "snippets" ? "暂无常用短语模板" : "暂无剪贴板历史记录"}
              </p>
              <span className="empty-sub">
                {query
                  ? "支持拼音首字母简拼（如 wx / sfz）或模糊匹配，输入 / 唤出短语"
                  : activeTab === "snippets"
                  ? "点击上方 [+ 新建] 预置高频常用回复或动态模板"
                  : "复制任意文本、代码或截屏图片后将自动捕获并在此显示"}
              </span>
            </div>
          ) : (
            displayItems.map((item, index) => {
              const isSelected = index === selectedIndex;
              const fastPasteIndex = index < 9 ? index + 1 : null;
              const isItemMultiSelected = selectedIds.includes(item.id);

              // 敏感信息临时显露判断
              const isSensitive = item.kind === "sensitive";
              const isTemporarilyRevealed = isSensitive && temporaryRevealId === item.id;
              const { displayText: maskedText } = maskSensitiveContent(item.content);
              const previewContent = isSensitive && !isTemporarilyRevealed ? maskedText : item.content;

              // 确定 28×28 槽位样式与图标
              let slotClass = "slot-text";
              let slotIcon: React.ReactNode = "📄";
              let badgeLabel = KIND_NAMES[item.kind];
              let badgeClass = "";

              if (item.kind === "sensitive") {
                slotClass = "slot-sensitive";
                slotIcon = "🛡️";
                badgeClass = "badge-sensitive";
              } else if (item.kind === "color") {
                slotClass = "slot-color";
                slotIcon = (
                  <div className="color-swatch-slot">
                    <div className="color-swatch-fill" style={{ backgroundColor: item.content.trim() }} />
                  </div>
                );
                badgeClass = "badge-color";
              } else if (item.kind === "code") {
                slotClass = "slot-code";
                slotIcon = "</>";
                badgeClass = "badge-code";
              } else if (item.kind === "link") {
                slotClass = "slot-link";
                slotIcon = "🔗";
                badgeClass = "badge-link";
              } else if (item.kind === "image") {
                slotClass = "slot-image";
                const imgDetail = imageDetails[item.content];
                slotIcon = imgDetail ? (
                  <img src={imgDetail.data_url} alt="缩略图" />
                ) : (
                  "🖼️"
                );
                badgeClass = "badge-image";
              } else if (item.kind === "snippet") {
                slotClass = "slot-snippet";
                slotIcon = "⚡";
                badgeClass = "badge-snippet";
              }

              return (
                <div
                  key={`${item.isSnippet ? "s" : "e"}-${item.id}`}
                  className={`panel-item ${isSelected ? "selected" : ""} ${item.is_pinned ? "pinned" : ""} ${
                    isItemMultiSelected ? "multi-selected" : ""
                  }`}
                  onClick={(e) => {
                    if (e.ctrlKey || e.metaKey) {
                      handleToggleSelectItem(item.id, e);
                    } else if (e.shiftKey) {
                      handleRangeSelectItem(index, e);
                    } else if (selectedIds.length > 0) {
                      handleToggleSelectItem(item.id, e);
                    } else {
                      handleDispatchPaste(item);
                    }
                  }}
                  onMouseEnter={() => setSelectedIndex(index)}
                >
                  <div className="item-leading">
                    {/* 1. 前置单槽位：平时显示数字键帽，悬停或多选时平滑原地置换为复选框 (Hover-Swap) */}
                    <div
                      className={`item-leading-slot ${fastPasteIndex ? "" : "dot"} ${
                        isItemMultiSelected ? "checked" : ""
                      } ${selectedIds.length > 0 ? "in-multi-mode" : ""}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        handleToggleSelectItem(item.id, e);
                      }}
                      title={
                        isItemMultiSelected
                          ? "取消勾选此项"
                          : selectedIds.length > 0
                          ? "勾选此项参与批量操作"
                          : fastPasteIndex
                          ? `快捷直贴: 直接按 ${fastPasteIndex} 或 Ctrl+${fastPasteIndex} (点击可勾选多选)`
                          : "点击勾选此项参与多选"
                      }
                    >
                      <span className="slot-num">{fastPasteIndex || "•"}</span>
                      <span className="slot-check">{isItemMultiSelected ? "✓" : ""}</span>
                    </div>

                    {/* 2. 绝对固定 28×28 槽位 */}
                    <div className={`item-slot ${slotClass}`}>{slotIcon}</div>

                    {/* 3. 标题与单行等宽摘要 (严格水平对齐) */}
                    <div className="item-content-box">
                      <div className="item-title-row">
                        <span className="item-title-text">{renderHighlightedText(item.title, query)}</span>
                        <span className={`item-badge-type ${badgeClass}`}>{badgeLabel}</span>
                        {item.shortcut && <span className="item-badge-type">/{item.shortcut}</span>}
                        {item.is_pinned && <span className="item-pin-star" title="已置顶">★</span>}
                        {isSensitive && (
                          <button
                            type="button"
                            className="slot-reveal-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleRevealSensitive(item.id);
                            }}
                            title="点击临时显露明文 (3秒自隐)"
                          >
                            {isTemporarilyRevealed ? "👁️ 显隐中" : "🔒 防窥"}
                          </button>
                        )}
                      </div>
                      <div className="item-snippet-snippet">
                        {item.kind === "image"
                          ? `[位图数据: ${imageDetails[item.content] ? `${imageDetails[item.content].width}×${imageDetails[item.content].height} • ${formatBytes(imageDetails[item.content].file_size)}` : "加载中..."}]`
                          : renderHighlightedText(previewContent, query)}
                      </div>
                    </div>
                  </div>

                  {/* 列表项右侧状态与快捷动作 */}
                  <div className="item-trailing">
                    <span className="item-time-label">
                      {item.kind === "image" && imageDetails[item.content]
                        ? formatBytes(imageDetails[item.content].file_size)
                        : formatRelativeTime(item.created_at)}
                    </span>
                    <div className="item-hover-actions">
                      <button
                        className="quick-action-btn"
                        onClick={(e) => {
                          e.stopPropagation();
                          setSelectedIndex(index);
                          setDrawerOpen(true);
                        }}
                        title="查看详细预览 (Space)"
                      >
                        Space 预览
                      </button>
                      <button
                        className="quick-action-btn danger"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDeleteItems([item]);
                        }}
                        title="从历史中删除 (Delete)"
                      >
                        🗑️
                      </button>
                      <button
                        className="quick-action-btn primary"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDispatchPaste(item);
                        }}
                        title="立即回填粘贴到当前活动窗口 (Enter)"
                      >
                        ↵ 回填
                      </button>
                    </div>
                  </div>
                </div>
              );
            })
          )}
        </div>

        {/* ========================================================================= */}
        {/* 即时抽屉检查器 (Quick Look Drawer - 按空格展开/收起)                        */}
        {/* ========================================================================= */}
        {drawerOpen && selectedItem && (
          <aside className="panel-drawer">
            <div>
              <div className="drawer-header">
                <div className="drawer-title-group">
                  <span className="drawer-title">{selectedItem.title}</span>
                  <span className="item-badge-type">{KIND_NAMES[selectedItem.kind]}</span>
                </div>
                <button
                  className="drawer-close-btn"
                  onClick={() => setDrawerOpen(false)}
                  title="收起预览 (Space/Esc)"
                >
                  ✕
                </button>
              </div>

              <div className="drawer-body">
                {/* 代码类型深度预览 */}
                {selectedItem.kind === "code" && (
                  <>
                    <pre className="drawer-code-block">{selectedItem.content}</pre>
                    <div className="drawer-actions-bar">
                      <button
                        className="drawer-mini-btn"
                        onClick={() => {
                          if (selectedItem.rawEntry) {
                            executeAction(selectedItem.rawEntry, "json_prettify");
                          }
                        }}
                      >
                        ✨ 美化 JSON
                      </button>
                      <button
                        className="drawer-mini-btn"
                        onClick={() => {
                          if (selectedItem.rawEntry) {
                            executeAction(selectedItem.rawEntry, "json_minify");
                          }
                        }}
                      >
                        📦 压缩单行
                      </button>
                      <button
                        className="drawer-mini-btn"
                        onClick={() => {
                          if (selectedItem.rawEntry) {
                            executeAction(selectedItem.rawEntry, "plain_text");
                          }
                        }}
                      >
                        📄 复制纯文本
                      </button>
                    </div>
                  </>
                )}

                {/* 链接类型深度预览 */}
                {selectedItem.kind === "link" && (
                  <div className="drawer-link-card">
                    <span className="drawer-link-url">{selectedItem.content}</span>
                    <button
                      className="drawer-mini-btn"
                      onClick={() => handleOpenLinkInBrowser(selectedItem.content)}
                    >
                      🌐 在默认浏览器中打开
                    </button>
                  </div>
                )}

                {/* 敏感信息防窥深度预览 */}
                {selectedItem.kind === "sensitive" && (
                  <div className="drawer-sensitive-card">
                    <div className="drawer-section-title amber">🔒 敏感信息防护</div>
                    <pre className="drawer-text-block">
                      {temporaryRevealId === selectedItem.id
                        ? selectedItem.content
                        : maskSensitiveContent(selectedItem.content).displayText}
                    </pre>
                    <button
                      className="drawer-mini-btn"
                      onClick={() => handleRevealSensitive(selectedItem.id)}
                    >
                      👁️ 临时显露明文 (3秒自隐)
                    </button>
                  </div>
                )}

                {/* 图片与 OCR 深度预览 */}
                {selectedItem.kind === "image" && (
                  <>
                    {imageDetails[selectedItem.content] && (
                      <img
                        src={imageDetails[selectedItem.content].data_url}
                        alt="大图"
                        className="drawer-image-preview"
                        onClick={() => setPreviewModalOpen(true)}
                        title="点击全屏放大"
                      />
                    )}
                    <div className="drawer-actions-bar">
                      <button
                        className="drawer-mini-btn"
                        onClick={() => handleOcr(selectedItem.id)}
                        disabled={ocrLoading}
                      >
                        {ocrLoading ? "⏳ 识别中..." : "🔍 提取文字 (OCR)"}
                      </button>
                      <button className="drawer-mini-btn" onClick={() => setPreviewModalOpen(true)}>
                        🔍 全屏放大预览
                      </button>
                    </div>
                    {ocrTextMap[selectedItem.id] && (
                      <div className="drawer-text-block">
                        <strong>OCR 提取结果：</strong>
                        <br />
                        {ocrTextMap[selectedItem.id]}
                      </div>
                    )}
                  </>
                )}

                {/* 常用短语模板预览 */}
                {selectedItem.kind === "snippet" && (
                  <div className="drawer-template-card">
                    <div className="drawer-section-title purple">⚡ 动态变量解析</div>
                    <pre className="drawer-text-block">{selectedItem.content}</pre>
                    <div className="drawer-actions-bar">
                      {selectedItem.rawSnippet && (
                        <>
                          <button
                            className="drawer-mini-btn"
                            onClick={(e) => handleOpenEditSnippet(selectedItem.rawSnippet!, e)}
                          >
                            ✏️ 编辑短语
                          </button>
                          <button
                            className="drawer-mini-btn"
                            onClick={(e) => handleDeleteSnippet(selectedItem.id, e)}
                          >
                            🗑️ 删除短语
                          </button>
                        </>
                      )}
                    </div>
                  </div>
                )}

                {/* 真实色彩方块嗅探与多格式互转深度卡片 (Ticket #26) */}
                {selectedItem.kind === "color" && (() => {
                  const colorObj = parseColor(selectedItem.content);
                  if (!colorObj) return <pre className="drawer-text-block">{selectedItem.content}</pre>;
                  return (
                    <div className="drawer-color-card">
                      <div className="drawer-color-preview-banner">
                        <div className="drawer-color-swatch-frame">
                          <div
                            className="drawer-color-swatch-fill"
                            style={{ backgroundColor: colorObj.raw }}
                          />
                        </div>
                        <div className="drawer-color-banner-info">
                          <span className="drawer-color-hex">{colorObj.hex}</span>
                          <span className="drawer-color-rgba">{colorObj.rgb}</span>
                        </div>
                      </div>
                      <div className="drawer-color-formats">
                        <div className="drawer-color-row">
                          <span className="color-format-label">HEX</span>
                          <span className="color-format-value">{colorObj.hex}</span>
                          <button
                            type="button"
                            className="drawer-mini-btn"
                            onClick={() => handleCopyColorValue(colorObj.hex)}
                          >
                            {copiedColorKey === colorObj.hex ? "✓ 已复制" : "📋 复制"}
                          </button>
                        </div>
                        <div className="drawer-color-row">
                          <span className="color-format-label">RGB</span>
                          <span className="color-format-value">{colorObj.rgb}</span>
                          <button
                            type="button"
                            className="drawer-mini-btn"
                            onClick={() => handleCopyColorValue(colorObj.rgb)}
                          >
                            {copiedColorKey === colorObj.rgb ? "✓ 已复制" : "📋 复制"}
                          </button>
                        </div>
                        <div className="drawer-color-row">
                          <span className="color-format-label">HSL</span>
                          <span className="color-format-value">{colorObj.hsl}</span>
                          <button
                            type="button"
                            className="drawer-mini-btn"
                            onClick={() => handleCopyColorValue(colorObj.hsl)}
                          >
                            {copiedColorKey === colorObj.hsl ? "✓ 已复制" : "📋 复制"}
                          </button>
                        </div>
                      </div>
                    </div>
                  );
                })()}

                {/* 普通纯文本预览 */}
                {selectedItem.kind === "text" && (
                  <pre className="drawer-text-block">{selectedItem.content}</pre>
                )}
              </div>
            </div>

            <div className="drawer-footer">
              <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                <span>
                  行数: {selectedItem.content.split("\n").length} • 大小:{" "}
                  {formatBytes(selectedItem.content.length)}
                </span>
                <button
                  className="drawer-mini-btn"
                  onClick={() => handleDeleteItems([selectedItem])}
                  title="从历史中删除此项 (Delete)"
                  style={{ padding: "1px 6px", fontSize: "11px", color: "#ef4444" }}
                >
                  🗑️ 删除
                </button>
              </div>
              <div style={{ display: "flex", gap: "6px" }}>
                {!selectedItem.isSnippet && selectedItem.kind !== "image" && (
                  <button
                    className="quick-action-btn"
                    onClick={() => handlePlainPaste(selectedItem.id)}
                    title="强制纯文本无格式粘贴 (Shift+Enter)"
                  >
                    ⇧↵ 纯文本
                  </button>
                )}
                <button
                  className="quick-action-btn primary"
                  onClick={() => handleDispatchPaste(selectedItem)}
                >
                  ↵ 粘贴 (Enter)
                </button>
              </div>
            </div>
          </aside>
        )}
      </div>

      {/* ========================================================================= */}
      {/* 底部状态栏 (Bottom Status Bar - 34px)                                       */}
      {/* ========================================================================= */}
      <footer className="panel-footer-bar" onMouseDown={handleStartDrag}>
        <div className="footer-left">
          <span className="status-dot" />
          <span style={{ fontWeight: 500 }}>
            {incognitoStatus.is_active ? "🕵️ 隐身无痕进行中" : "监听就绪"}
          </span>
          <span style={{ color: "#cbd5e1" }}>|</span>
          <button
            className={`queue-pill-btn ${queueStatus.is_active ? "active" : ""}`}
            onClick={handleTogglePasteQueue}
            title="快捷键: Alt+Shift+C"
          >
            {queueStatus.is_active ? `⚡ 连贴运行中 (${queueStatus.count}项)` : "连贴收集 (Alt+Shift+C)"}
          </button>
        </div>

        <div className="footer-right">
          <button
            className="footer-action-palette-btn"
            onClick={() => {
              if (selectedItem && !selectedItem.isSnippet) {
                setActionSelectedIndex(0);
                setActionPaletteOpen(true);
              }
            }}
            title="快捷格式转换 (Tab / Ctrl+K)"
          >
            <span>⚡ Tab 动作面板</span>
          </button>

          <div className="footer-nav-hints">
            <span>1~9 / Ctrl+1~9 直贴</span>
            <span>/ 搜索</span>
            <span>↑↓ 漫游</span>
            <span>Space 抽屉</span>
            <span>↵ 回填</span>
            <span>⇧↵ 纯文本</span>
          </div>
        </div>
      </footer>

      {/* ========================================================================= */}
      {/* 多选合并回填与批量删除浮动工具栏                                           */}
      {/* ========================================================================= */}
      {selectedIds.length > 0 && (
        <div className="multi-select-toolbar">
          <div className="multi-select-info">
            <span className="multi-select-badge">{selectedIds.length}</span>
            <span>已选 {selectedIds.length} 项 (Delete 删除，Enter 粘贴，Esc 取消)</span>
          </div>
          <div className="multi-select-actions">
            <button
              className="multi-btn danger"
              onClick={() => {
                const targets = displayItems.filter((i) => selectedIds.includes(i.id));
                handleDeleteItems(targets);
              }}
              title="批量删除所选条目 (Delete)"
            >
              🗑️ 批量删除 ({selectedIds.length})
            </button>
            <button className="multi-btn primary" onClick={() => handlePasteMultiple(selectedIds)} title="合并粘贴到前台窗口 (Enter)">
              ↵ 合并粘贴
            </button>
            <button className="multi-btn" onClick={() => setSelectedIds([])} title="取消选择 (Esc)">
              取消
            </button>
          </div>
        </div>
      )}

      {/* ========================================================================= */}
      {/* 动作调色板浮层 (Action Palette - Tab / Ctrl+K)                              */}
      {/* ========================================================================= */}
      {actionPaletteOpen && selectedItem?.rawEntry && (
        <div className="action-palette-overlay" onClick={() => setActionPaletteOpen(false)}>
          <div className="action-palette" onClick={(e) => e.stopPropagation()}>
            <div className="palette-header">
              <div className="palette-title">
                <span>⚡ 格式清洗与动作调色板 (Action Palette)</span>
              </div>
              {transformError ? (
                <div className="palette-error">⚠️ {transformError}</div>
              ) : (
                <div className="palette-sub">目标: {selectedItem.title}</div>
              )}
            </div>
            <div className="palette-list">
              {(selectedItem.kind === "image" ? IMAGE_ACTIONS : TRANSFORM_ACTIONS).map((action, idx) => {
                const isActionSelected = idx === actionSelectedIndex;
                return (
                  <div
                    key={action.key}
                    className={`palette-item ${isActionSelected ? "selected" : ""}`}
                    onClick={() => {
                      if (selectedItem.rawEntry) executeAction(selectedItem.rawEntry, action.key);
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
              <span>
                <kbd>↵</kbd> / <kbd>{(selectedItem.kind === "image" ? IMAGE_ACTIONS : TRANSFORM_ACTIONS).length > 1 ? `1~${(selectedItem.kind === "image" ? IMAGE_ACTIONS : TRANSFORM_ACTIONS).length}` : "1"}</kbd> 执行并回填
              </span>
              <span>
                <kbd>Esc</kbd> 取消返回
              </span>
            </div>
          </div>
        </div>
      )}

      {/* ========================================================================= */}
      {/* 大图全屏预览弹窗                                                           */}
      {/* ========================================================================= */}
      {previewModalOpen && selectedItem?.kind === "image" && (
        <div className="image-preview-overlay" onClick={() => setPreviewModalOpen(false)}>
          <div className="image-preview-modal" onClick={(e) => e.stopPropagation()}>
            <div className="image-preview-header">
              <div className="image-preview-title">
                <span>🖼️ 位图高清大图预览</span>
              </div>
              <button className="image-preview-close" onClick={() => setPreviewModalOpen(false)}>
                ✕
              </button>
            </div>
            <div className="image-preview-modal-body">
              {imageDetails[selectedItem.content] && (
                <img
                  src={imageDetails[selectedItem.content].data_url}
                  alt="大图预览"
                  className="image-preview-modal-img"
                />
              )}
            </div>
            <div className="snippet-modal-footer">
              <button className="btn-cancel" onClick={() => setPreviewModalOpen(false)}>
                关闭
              </button>
              <button className="btn-primary" onClick={() => handlePaste(selectedItem.id)}>
                ↵ 粘贴图片
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ========================================================================= */}
      {/* 常用短语新建/编辑模态框                                                    */}
      {/* ========================================================================= */}
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
              {snippetFormError && <div className="palette-error">⚠️ {snippetFormError}</div>}
              <div className="form-group">
                <label className="form-label">
                  短语标题 <span className="req">*</span>
                </label>
                <input
                  type="text"
                  className="form-input"
                  value={snippetTitle}
                  onChange={(e) => setSnippetTitle(e.target.value)}
                  placeholder="例如: 站会汇报模板、常用联系方式"
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
                    placeholder="如 meet、info、cr"
                  />
                </div>
              </div>
              <div className="form-group">
                <label className="form-label">模板内容 <span className="req">*</span></label>
                <div className="placeholder-toolbar">
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{current_date}")}>
                    + &#123;current_date&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{time}")}>
                    + &#123;time&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{datetime}")}>
                    + &#123;datetime&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{clipboard}")}>
                    + &#123;clipboard&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{year}")}>
                    + &#123;year&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{month}")}>
                    + &#123;month&#125;
                  </button>
                  <button type="button" className="ph-btn" onClick={() => insertPlaceholder("{day}")}>
                    + &#123;day&#125;
                  </button>
                </div>
                <textarea
                  ref={textareaRef}
                  className="form-textarea"
                  rows={5}
                  value={snippetContent}
                  onChange={(e) => setSnippetContent(e.target.value)}
                  placeholder="请输入短语内容模板..."
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

      {/* ========================================================================= */}
      {/* 系统设置与灾备管理模态框                                                   */}
      {/* ========================================================================= */}
      {settingsModalOpen && (
        <div className="settings-overlay" onClick={() => setSettingsModalOpen(false)}>
          <div className="settings-modal" onClick={(e) => e.stopPropagation()}>
            <div className="settings-header">
              <div className="settings-title">
                <span>⚙️ 系统设置与灾备管理</span>
              </div>
              <button className="settings-close-btn" onClick={() => setSettingsModalOpen(false)}>
                ✕
              </button>
            </div>
            <div className="settings-body">
              {/* 1. 快捷键中心 (Shortcut Center) */}
              <div className="settings-section">
                <div className="settings-section-title">⌨️ 快捷键中心</div>
                <div className="settings-desc" style={{ marginBottom: "10px" }}>
                  配置系统级全局唤起热键，并快速查阅面板内置高频交互快捷键全景图谱。
                </div>
                <div className="shortcut-config-box">
                  <div className="shortcut-current-row">
                    <span className="settings-label">系统全局唤起热键：</span>
                    <kbd className="shortcut-badge">{globalShortcut}</kbd>
                  </div>
                  <div className="shortcut-recorder-row">
                    <div className="shortcut-input-container">
                      <input
                        type="text"
                        className={`shortcut-record-input ${isRecordingShortcut ? "recording" : ""}`}
                        value={isRecordingShortcut ? (shortcutDraft ? `${shortcutDraft} (录制中...)` : "请按下组合键...") : shortcutDraft}
                        placeholder="点击后按下按键组合 (如 Ctrl+Shift+V)"
                        readOnly
                        onFocus={() => {
                          setIsRecordingShortcut(true);
                          setShortcutFeedback(null);
                        }}
                        onBlur={() => setIsRecordingShortcut(false)}
                        onKeyDown={handleShortcutKeyDown}
                      />
                      {isRecordingShortcut && <span className="recording-indicator">● 录制中</span>}
                    </div>
                    <button
                      className="btn-shortcut-save"
                      onClick={() => handleSaveShortcut(shortcutDraft)}
                      disabled={shortcutDraft === globalShortcut}
                    >
                      保存生效
                    </button>
                    <button
                      className="btn-shortcut-reset"
                      onClick={handleResetDefaultShortcut}
                      title="恢复默认快捷键 (Alt+V)"
                    >
                      恢复默认
                    </button>
                  </div>
                  {shortcutFeedback && (
                    <div className={`shortcut-feedback-msg ${shortcutFeedback.type}`}>
                      {shortcutFeedback.type === "success" ? "✓ " : "⚠️ "}
                      {shortcutFeedback.text}
                    </div>
                  )}
                </div>

                {/* 交互快捷键全景速查卡片 (Cheat Sheet) */}
                <div className="shortcuts-cheatsheet-card">
                  <div className="cheatsheet-category">
                    <span className="cheatsheet-cat-title">📋 极速粘贴与格式</span>
                    <div className="cheatsheet-grid">
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">立即回填选中项</span>
                        <div className="cheatsheet-kbd-group"><kbd>Enter</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">纯文本无格式粘贴</span>
                        <div className="cheatsheet-kbd-group"><kbd>Shift</kbd> + <kbd>Enter</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">前 9 项极速直贴</span>
                        <div className="cheatsheet-kbd-group"><kbd>1~9</kbd> 或 <kbd>Ctrl</kbd> + <kbd>1~9</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">连贴模式启动/暂停</span>
                        <div className="cheatsheet-kbd-group"><kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>C</kbd></div>
                      </div>
                    </div>
                  </div>

                  <div className="cheatsheet-category">
                    <span className="cheatsheet-cat-title">🗂️ 列表导航与窗口</span>
                    <div className="cheatsheet-grid">
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">激活搜索打字</span>
                        <div className="cheatsheet-kbd-group"><kbd>/</kbd> 或 鼠标点击</div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">上下漫游浏览历史</span>
                        <div className="cheatsheet-kbd-group"><kbd>↑</kbd> / <kbd>↓</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">窗口钉住/保持固定</span>
                        <div className="cheatsheet-kbd-group"><kbd>Alt</kbd> + <kbd>P</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">展开/收起详情抽屉</span>
                        <div className="cheatsheet-kbd-group"><kbd>Space</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">唤出动作格式转换面板</span>
                        <div className="cheatsheet-kbd-group"><kbd>Ctrl</kbd> + <kbd>K</kbd> / <kbd>Tab</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">切换历史与短语 Tab</span>
                        <div className="cheatsheet-kbd-group"><kbd>Ctrl</kbd> + <kbd>Tab</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">逐级退出/隐藏面板</span>
                        <div className="cheatsheet-kbd-group"><kbd>Esc</kbd></div>
                      </div>
                    </div>
                  </div>

                  <div className="cheatsheet-category">
                    <span className="cheatsheet-cat-title">🛡️ 条目治理与多选</span>
                    <div className="cheatsheet-grid">
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">物理删除单选/多选项</span>
                        <div className="cheatsheet-kbd-group"><kbd>Delete</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">撤销删除 (3秒网)</span>
                        <div className="cheatsheet-kbd-group"><kbd>Ctrl</kbd> + <kbd>Z</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">置顶/取消置顶条目</span>
                        <div className="cheatsheet-kbd-group"><kbd>Ctrl</kbd> + <kbd>P</kbd></div>
                      </div>
                      <div className="cheatsheet-item">
                        <span className="cheatsheet-desc">新建常用短语模板</span>
                        <div className="cheatsheet-kbd-group"><kbd>Ctrl</kbd> + <kbd>N</kbd></div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              {/* 2. 窗口行为与固定置顶 (WindowPinning) */}
              <div className="settings-section">
                <div className="settings-section-title">🖥️ 窗口交互行为</div>
                <div className="settings-row">
                  <div className="settings-label-group">
                    <span className="settings-label">失去焦点时自动隐藏窗口</span>
                    <span className="settings-desc">在未钉住状态下，点击外部其他程序或桌面时自动收起面板</span>
                  </div>
                  <label className="toggle-checkbox-label">
                    <input
                      type="checkbox"
                      checked={autoHideOnBlur}
                      onChange={handleToggleAutoHideOnBlur}
                    />
                  </label>
                </div>
                <div className="settings-row" style={{ marginTop: "10px" }}>
                  <div className="settings-label-group">
                    <span className="settings-label">窗口钉住/保持固定 (Alt+P)</span>
                    <span className="settings-desc">
                      <span className={`status-indicator-dot ${isPinned ? "active" : ""}`} />
                      {isPinned
                        ? "当前已处于固定状态，点击任何外部程序窗口均常驻不消失"
                        : "未固定，窗口将在失焦时根据上方规则收起"}
                    </span>
                  </div>
                  <button
                    className={`btn-pin-toggle ${isPinned ? "pinned" : ""}`}
                    onClick={handleToggleWindowPin}
                  >
                    {isPinned ? "📌 取消固定" : "📌 立即固定"}
                  </button>
                </div>
              </div>

              <div className="settings-section">
                <div className="settings-section-title">🚀 系统守护</div>
                <div className="settings-row">
                  <div className="settings-label-group">
                    <span className="settings-label">开机静默后台自启</span>
                    <span className="settings-desc">随系统开机在后台静默运行（进入托盘不弹窗）</span>
                  </div>
                  <label>
                    <input type="checkbox" checked={autostartEnabled} onChange={handleToggleAutostart} />
                  </label>
                </div>
                <div className="settings-row" style={{ marginTop: "10px" }}>
                  <div className="settings-label-group">
                    <span className="settings-label">无痕私密模式</span>
                    <span className="settings-desc">
                      <span className={`status-indicator-dot ${incognitoStatus.is_active ? "active" : ""}`} />
                      {incognitoStatus.is_active ? "当前无痕模式已启用，暂停记录所有新剪贴内容" : "未开启，正常记录剪贴历史"}
                    </span>
                  </div>
                  <button
                    className={`btn-pin-toggle ${incognitoStatus.is_active ? "pinned" : ""}`}
                    onClick={() => handleToggleIncognito()}
                  >
                    {incognitoStatus.is_active ? "退出无痕" : "进入无痕"}
                  </button>
                </div>
              </div>

              {/* 4. 数据存储与目录定位 (Issue #19) */}
              <div className="settings-section">
                <div className="settings-section-title">📂 数据存储与占用</div>
                <div className="settings-row">
                  <div className="settings-label-group">
                    <span className="settings-label">存储数据目录</span>
                    <span className="settings-desc">
                      {storageInfo ? (
                        <>
                          <span className="storage-path-text" title={storageInfo.db_path}>
                            {storageInfo.db_path}
                          </span>
                          <span style={{ marginLeft: "8px", color: "var(--brand-primary)", cursor: "pointer" }} onClick={() => handleCopyStoragePath(storageInfo.db_path)}>
                            {copyPathFeedback ? "✓ 已复制" : "📋 复制"}
                          </span>
                        </>
                      ) : (
                        "正在计算数据存储位置..."
                      )}
                    </span>
                  </div>
                  <button className="btn-storage-open" onClick={handleOpenStorageDir} title="在 Windows 资源管理器中打开当前数据目录">
                    📂 打开目录
                  </button>
                </div>
                {storageInfo && (
                  <div className="storage-stats-bar">
                    <div className="storage-stat-pill">
                      <span className="stat-pill-label">数据库</span>
                      <span className="stat-pill-value">{formatBytes(storageInfo.db_size_bytes)}</span>
                    </div>
                    <div className="storage-stat-pill">
                      <span className="stat-pill-label">图片缓存</span>
                      <span className="stat-pill-value">{formatBytes(storageInfo.blob_size_bytes)}</span>
                    </div>
                    <div className="storage-stat-pill">
                      <span className="stat-pill-label">历史记录</span>
                      <span className="stat-pill-value">{storageInfo.total_entries} 条</span>
                    </div>
                    <div className="storage-stat-pill">
                      <span className="stat-pill-label">常用短语</span>
                      <span className="stat-pill-value">{storageInfo.total_snippets} 条</span>
                    </div>
                  </div>
                )}
                <div className="settings-row" style={{ marginTop: "12px" }}>
                  <div className="settings-label-group">
                    <span className="settings-label">历史容量上限 (LRU 自动淘汰)</span>
                    <span className="settings-desc">超额时自动清理最旧未置顶条目与孤立图片（置顶项永久豁免）</span>
                  </div>
                  <div className="capacity-btn-group">
                    {[50, 200, 500, 1000].map((cap) => (
                      <button
                        key={cap}
                        type="button"
                        className={`capacity-pill-btn ${historyCapacity === cap ? "active" : ""}`}
                        onClick={() => handleSetCapacity(cap)}
                        title={`设置历史容量为 ${cap} 条`}
                      >
                        {cap} 条
                      </button>
                    ))}
                  </div>
                </div>
              </div>

              {/* 5. 原生灾备归档 (.clipbak) (Issue #19) */}
              <div className="settings-section">
                <div className="settings-section-title">📦 灾备归档 (.clipbak)</div>
                <div className="settings-desc" style={{ marginBottom: "10px" }}>
                  支持将历史剪贴板记录、图片 Blob 与常用短语完整归档打包与一键还原迁移。
                </div>
                <div className="backup-btn-group">
                  <button className="backup-action-btn" disabled={backupLoading} onClick={handleExportBackup}>
                    {backupLoading ? "导出中..." : "📤 一键导出备份 (另存为)"}
                  </button>
                  <button className="backup-action-btn" disabled={backupLoading} onClick={handleImportBackup}>
                    {backupLoading ? "导入中..." : "📥 导入备份还原 (合并)"}
                  </button>
                </div>
                {backupMsg && <div className={`backup-feedback ${backupMsg.type}`}>{backupMsg.text}</div>}
              </div>

              {/* 6. 关于与版本更新 (Issue #19) */}
              <div className="settings-section">
                <div className="settings-section-title">🌐 关于与检查更新</div>
                <div className="settings-row">
                  <div className="settings-label-group">
                    <span className="settings-label">
                      Clip 剪贴板管理器 <span className="version-pill">{CURRENT_VERSION}</span>
                    </span>
                    <span className="settings-desc">极轻量、跨平台桌面剪贴板历史管理器 (Tauri v2 + React + Rust)</span>
                  </div>
                  <button className="btn-update-check" disabled={updateChecking} onClick={() => handleCheckUpdate(true)}>
                    {updateChecking ? "检查中..." : "🔍 检查更新"}
                  </button>
                </div>
                {updateFeedback && (
                  <div className={`update-feedback ${updateFeedback.type}`}>
                    {updateFeedback.type === "success" ? "✓ " : "⚠️ "}
                    {updateFeedback.text}
                  </div>
                )}
              </div>

              {/* 7. 清理维护 */}
              <div className="settings-section">
                <div className="settings-section-title">🧹 清理维护</div>
                <div className="settings-row">
                  <div className="settings-label-group">
                    <span className="settings-label">清空剪贴板历史</span>
                    <span className="settings-desc">清除所有已捕获历史与图片缓存（常用短语将保留）</span>
                  </div>
                  <button className="danger-clear-btn" onClick={handleClearAllHistory}>
                    清空历史
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ========================================================================= */}
      {/* 发现新版本更新提示模态框 (Issue #19 & #20)                                   */}
      {/* ========================================================================= */}
      {updateModalOpen && updateInfo && (
        <div
          className="update-modal-overlay"
          onClick={() => {
            if (!downloadingUpdate) setUpdateModalOpen(false);
          }}
        >
          <div className="update-modal-card" onClick={(e) => e.stopPropagation()}>
            <div className="update-modal-header">
              <div className="update-modal-title">
                <span>🚀 发现新版本可用</span>
                <span className="update-new-version-badge">{updateInfo.tag_name}</span>
              </div>
              {!downloadingUpdate && (
                <button className="update-modal-close-btn" onClick={() => setUpdateModalOpen(false)}>
                  ✕
                </button>
              )}
            </div>
            <div className="update-modal-body">
              <div className="update-release-title">{updateInfo.name || `Clip ${updateInfo.tag_name}`}</div>
              <div className="update-release-date">
                发布日期：{updateInfo.published_at ? updateInfo.published_at.slice(0, 10) : "最新"}
              </div>
              <div className="update-notes-container">
                <div className="update-notes-label">更新日志 (Release Notes)：</div>
                <ReleaseNotesView body={updateInfo.body} />
              </div>

              {/* 流式下载进度展示 (Issue #20) */}
              {downloadingUpdate && (
                <div className="update-progress-section">
                  <div className="update-progress-header">
                    <span>{downloadProgress.statusText}</span>
                    <span className="update-progress-percent">{downloadProgress.percent}%</span>
                  </div>
                  <div className="update-progress-bar-bg">
                    <div
                      className="update-progress-bar-fill"
                      style={{ width: `${downloadProgress.percent}%` }}
                    />
                  </div>
                  <div className="update-progress-footer">
                    <span>
                      {downloadProgress.received > 0
                        ? `${(downloadProgress.received / (1024 * 1024)).toFixed(1)} MB`
                        : ""}
                      {downloadProgress.total > 0
                        ? ` / ${(downloadProgress.total / (1024 * 1024)).toFixed(1)} MB`
                        : ""}
                    </span>
                    <span>原地自愈覆盖 (免卸载)</span>
                  </div>
                </div>
              )}

              {/* 下载错误提示与直链下载救济 */}
              {downloadError && (
                <div className="update-error-banner">
                  <div className="update-error-message">⚠️ {downloadError}</div>
                  {updateInfo.assets && updateInfo.assets.length > 0 && (
                    <button
                      className="btn-update-browser-direct"
                      onClick={() => {
                        const setupAsset =
                          updateInfo.assets?.find((a) => a.name.includes("Windows") && a.name.endsWith(".exe")) ||
                          updateInfo.assets?.find((a) => a.name.endsWith("-Setup.exe") || a.name.endsWith(".exe"));
                        if (setupAsset) {
                          openUrl(setupAsset.browser_download_url);
                        } else if (updateInfo.html_url) {
                          openUrl(updateInfo.html_url);
                        }
                      }}
                    >
                      🌐 在浏览器中直接下载安装包
                    </button>
                  )}
                </div>
              )}
            </div>
            <div className="update-modal-footer">
              {downloadingUpdate ? (
                <button className="btn-update-dismiss" onClick={handleCancelUpdateDownload}>
                  ✕ 取消下载
                </button>
              ) : (
                <>
                  <button className="btn-update-dismiss" onClick={() => setUpdateModalOpen(false)}>
                    暂不更新
                  </button>
                  <button
                    className="btn-update-download"
                    onClick={() => {
                      if (updateInfo.html_url) {
                        openUrl(updateInfo.html_url);
                      }
                      setUpdateModalOpen(false);
                    }}
                  >
                    🌐 前往 GitHub
                  </button>
                  <button className="btn-update-primary" onClick={handleStartInPlaceUpdate}>
                    🚀 一键下载并安装
                  </button>
                </>
              )}
            </div>
          </div>
        </div>
      )}

      {/* 3 秒撤销通知悬浮气泡 (Safety Net) */}
      {undoToast && (
        <div className="undo-toast-bubble">
          <span>{undoToast.text}</span>
          {undoToast.showUndo && (
            <button className="undo-toast-btn" onClick={handleUndoDelete}>
              撤销 (Ctrl+Z)
            </button>
          )}
        </div>
      )}
    </div>
  );
};

/**
 * 根组件：物理分流独立 HUD 胶囊视图与主程序面板
 */
export const App: React.FC = () => {
  const isHudView = typeof window !== "undefined" && window.location.search.includes("view=hud");
  if (isHudView) {
    return <StandaloneHudView />;
  }
  return <MainPanel />;
};

export default App;
