/**
 * 语义化版本号解析与比对工具
 *
 * 用于客户端版本检查判定，支持解析带有 'v' 前缀的 SemVer 版本号并逐级比对。
 *
 * @author Ateng
 * @since 2026-10-08
 */

/**
 * 语义化版本比对纯函数
 *
 * @param current 当前客户端版本 (如 "1.2.1" 或 "v1.2.1")
 * @param remote 远程发布版本 (如 "1.2.2" 或 "v1.2.2")
 * @return 1 若 remote > current (发现新版本)；0 若版本一致；-1 若 remote < current
 */
export function compareSemVer(current: string, remote: string): number {
  const parseParts = (v: string): number[] => {
    return v
      .trim()
      .replace(/^v/i, "")
      .split(/[-+]/)[0] // 剥离预发布标签如 -beta
      .split(".")
      .map((part) => {
        const num = parseInt(part, 10);
        return isNaN(num) ? 0 : num;
      });
  };

  const curParts = parseParts(current);
  const remParts = parseParts(remote);
  const maxLen = Math.max(curParts.length, remParts.length, 3);

  for (let i = 0; i < maxLen; i++) {
    const c = curParts[i] ?? 0;
    const r = remParts[i] ?? 0;
    if (r > c) return 1;
    if (r < c) return -1;
  }

  return 0;
}

/**
 * 将字节大小格式化为直观的人类可读文本 (如 2.4 MB)
 *
 * @param bytes 字节大小数值
 * @return 格式化后的字符串
 */
export function formatBytes(bytes: number): string {
  if (bytes <= 0 || isNaN(bytes)) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const idx = Math.min(i, sizes.length - 1);
  return `${parseFloat((bytes / Math.pow(k, idx)).toFixed(1))} ${sizes[idx]}`;
}
