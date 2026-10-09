import React, { useMemo } from "react";

/**
 * GitHub Release 变更日志结构化富文本渲染组件。
 *
 * 将 GitHub Releases 的原始 Markdown 正文解析为分组徽章、结构化卡片与行内代码标签，
 * 消除生硬的 Markdown 语法符号，提供清晰易读的人机交互呈现。
 *
 * @author Ateng
 * @since 2026-10-08
 */

interface ReleaseNotesViewProps {
  /** 原始 Markdown 正文内容 */
  body?: string | null;
}

interface NoteItem {
  id: string;
  title: string;
  description: string;
}

interface NoteSection {
  id: string;
  category: string;
  icon: string;
  badgeClass: string;
  items: NoteItem[];
}

/**
 * 将行内反引号文本转为 React 元素数组
 */
function renderInlineContent(text: string): React.ReactNode {
  const parts = text.split(/(`[^`]+`)/g);
  return parts.map((part, index) => {
    if (part.startsWith("`") && part.endsWith("`")) {
      return (
        <code key={index} className="release-note-code">
          {part.slice(1, -1)}
        </code>
      );
    }
    return part;
  });
}

/**
 * 解析分类标题并映射为语义化徽章
 */
function matchCategory(rawTitle: string): { category: string; icon: string; badgeClass: string } {
  const lower = rawTitle.toLowerCase();
  if (lower.includes("feature") || lower.includes("特性") || lower.includes("新功能")) {
    return { category: "功能特性", icon: "🌟", badgeClass: "badge-feature" };
  }
  if (lower.includes("fix") || lower.includes("修复") || lower.includes("bug")) {
    return { category: "缺陷修复", icon: "🐛", badgeClass: "badge-fix" };
  }
  if (lower.includes("perf") || lower.includes("性能") || lower.includes("优化")) {
    return { category: "性能优化", icon: "⚡", badgeClass: "badge-perf" };
  }
  if (lower.includes("refactor") || lower.includes("重构")) {
    return { category: "架构重构", icon: "🛠️", badgeClass: "badge-refactor" };
  }
  if (lower.includes("doc") || lower.includes("文档")) {
    return { category: "文档更新", icon: "📖", badgeClass: "badge-doc" };
  }
  return { category: rawTitle.replace(/^#+\s*/, "").trim(), icon: "📌", badgeClass: "badge-default" };
}

export const ReleaseNotesView: React.FC<ReleaseNotesViewProps> = ({ body }) => {
  const sections = useMemo<NoteSection[]>(() => {
    if (!body || !body.trim()) {
      return [];
    }

    const lines = body.split("\n");
    const result: NoteSection[] = [];
    let currentSection: NoteSection = {
      id: "section-init",
      category: "更新摘要",
      icon: "✨",
      badgeClass: "badge-default",
      items: [],
    };

    let itemCounter = 0;

    for (const rawLine of lines) {
      const line = rawLine.trim();
      if (!line) continue;

      // 1. 匹配 Markdown 三级及以上标题: ### Features 等
      if (line.startsWith("###") || line.startsWith("##")) {
        if (currentSection.items.length > 0) {
          result.push(currentSection);
        }
        const { category, icon, badgeClass } = matchCategory(line);
        currentSection = {
          id: `section-${result.length}-${category}`,
          category,
          icon,
          badgeClass,
          items: [],
        };
        continue;
      }

      // 2. 匹配列表项: * 或 - 开头
      if (line.startsWith("*") || line.startsWith("-")) {
        const itemContent = line.replace(/^[\*\-]\s+/, "");
        itemCounter++;

        // 尝试提取 **标题**: 内容
        const boldMatch = itemContent.match(/^\*\*([^\*]+)\*\*[:：]?\s*(.*)$/);
        if (boldMatch) {
          currentSection.items.push({
            id: `item-${itemCounter}`,
            title: boldMatch[1].trim(),
            description: boldMatch[2].trim(),
          });
        } else {
          currentSection.items.push({
            id: `item-${itemCounter}`,
            title: "",
            description: itemContent.replace(/\*\*/g, "").trim(),
          });
        }
        continue;
      }

      // 3. 其他补充说明行（段落或引用）
      if (!line.startsWith("#")) {
        itemCounter++;
        const cleanLine = line.replace(/^>\s*/, "").replace(/\*\*/g, "").trim();
        currentSection.items.push({
          id: `item-${itemCounter}`,
          title: "",
          description: cleanLine,
        });
      }
    }

    if (currentSection.items.length > 0) {
      result.push(currentSection);
    }

    return result;
  }, [body]);

  if (sections.length === 0) {
    return (
      <div className="release-notes-empty">
        <span className="release-notes-empty-icon">📝</span>
        <span>本次发布包含常规稳定性改进与体验优化。</span>
      </div>
    );
  }

  return (
    <div className="release-notes-view">
      {sections.map((section) => (
        <div key={section.id} className="release-section">
          <div className={`release-section-badge ${section.badgeClass}`}>
            <span className="section-badge-icon">{section.icon}</span>
            <span className="section-badge-text">{section.category}</span>
          </div>

          <div className="release-items-list">
            {section.items.map((item) => (
              <div key={item.id} className="release-item-card">
                <span className="release-item-bullet">•</span>
                <div className="release-item-content">
                  {item.title && (
                    <span className="release-item-title">
                      {renderInlineContent(item.title)}
                      {item.description ? "：" : ""}
                    </span>
                  )}
                  <span className="release-item-desc">
                    {renderInlineContent(item.description)}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
};
