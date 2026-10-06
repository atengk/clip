/**
 * 敏感凭据防窥嗅探与脱敏打码工具库 (Masked View)。
 *
 * 负责手机号、身份证、银行卡及常见 API Token 的规则嗅探与脱敏遮蔽。
 *
 * @author Ateng
 * @since 2026-10-06
 */

/**
 * 敏感字段脱敏结果结构
 */
export interface MaskedResult {
  displayText: string;
  isSensitive: boolean;
}

/**
 * 针对手机号、身份证号、银行卡号及常见 API Token 执行界面脱敏打码
 *
 * @param text 原始内容
 * @return 脱敏后的展示文本与敏感判定标志
 */
export function maskSensitiveContent(text: string): MaskedResult {
  const trimmed = text.trim();
  let masked = text;
  let detected = false;

  // 1. 独立单值精准脱敏优先匹配
  // (1) 常见 API Token (以 sk- 或 ghp_ 开头且长度 >= 24)
  if ((trimmed.startsWith("sk-") || trimmed.startsWith("ghp_")) && trimmed.length >= 24) {
    const prefix = trimmed.startsWith("sk-") ? "sk-" : "ghp_";
    const suffix = trimmed.slice(-4);
    return { displayText: `${prefix}****${suffix}`, isSensitive: true };
  }

  // (2) 18 位中国大陆身份证号精确脱敏 (前6后4)
  if (trimmed.length === 18 && /^\d{17}[\dXx]$/.test(trimmed)) {
    return { displayText: `${trimmed.slice(0, 6)}********${trimmed.slice(14)}`, isSensitive: true };
  }

  // (3) 16~19 位银行卡号精确脱敏 (前4后4)
  if (/^\d{16,19}$/.test(trimmed)) {
    return { displayText: `${trimmed.slice(0, 4)} **** **** ${trimmed.slice(-4)}`, isSensitive: true };
  }

  // (4) 11 位中国大陆手机号精确脱敏
  if (/^1\d{10}$/.test(trimmed)) {
    return { displayText: `${trimmed.slice(0, 3)}****${trimmed.slice(7)}`, isSensitive: true };
  }

  // 2. 文本内嵌敏感信息模式替换
  // (1) 内嵌 API Token 替换
  const tokenRegex = /(sk-[a-zA-Z0-9_\-]{20,}|ghp_[a-zA-Z0-9]{20,})/g;
  if (tokenRegex.test(masked)) {
    masked = masked.replace(tokenRegex, (match) => {
      const prefix = match.startsWith("sk-") ? "sk-" : "ghp_";
      return `${prefix}****${match.slice(-4)}`;
    });
    detected = true;
  }

  // (2) 内嵌 18 位身份证替换
  const idRegex = /(?<![0-9a-zA-Z])(\d{6})\d{8}(\d{3}[\dXx])(?![0-9a-zA-Z])/g;
  if (idRegex.test(masked)) {
    masked = masked.replace(idRegex, "$1********$2");
    detected = true;
  }

  // (3) 内嵌 11 位手机号替换
  const phoneRegex = /(?<!\d)(1\d{2})\d{4}(\d{4})(?!\d)/g;
  if (phoneRegex.test(masked)) {
    masked = masked.replace(phoneRegex, "$1****$2");
    detected = true;
  }

  return { displayText: masked, isSensitive: detected };
}
