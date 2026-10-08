/**
 * 色彩嗅探与多格式互转工具库 (Smart Color Detector)。
 *
 * 负责 HEX、RGB/RGBA、HSL/HSLA 色彩格式嗅探识别与数值标准化双向互转。
 *
 * @author Ateng
 * @since 2026-10-08
 */

/**
 * 标准化色彩信息实体契约
 */
export interface ColorInfo {
  isValid: boolean;
  raw: string;
  r: number;
  g: number;
  b: number;
  a: number;
  hex: string;
  rgb: string;
  hsl: string;
}

/**
 * 限制数值在 [min, max] 区间闭包内
 */
function clamp(val: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, val));
}

/**
 * RGB 转 HSL 换算
 */
export function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number } {
  const normR = r / 255;
  const normG = g / 255;
  const normB = b / 255;
  const max = Math.max(normR, normG, normB);
  const min = Math.min(normR, normG, normB);
  let h = 0;
  let s = 0;
  const l = (max + min) / 2;

  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case normR:
        h = (normG - normB) / d + (normG < normB ? 6 : 0);
        break;
      case normG:
        h = (normB - normR) / d + 2;
        break;
      case normB:
        h = (normR - normG) / d + 4;
        break;
    }
    h = Math.round(h * 60);
  }

  return {
    h: (h % 360 + 360) % 360,
    s: Math.round(s * 100),
    l: Math.round(l * 100),
  };
}

/**
 * HSL 转 RGB 换算
 */
export function hslToRgb(h: number, s: number, l: number): { r: number; g: number; b: number } {
  const normH = ((h % 360) + 360) % 360;
  const normS = clamp(s, 0, 100) / 100;
  const normL = clamp(l, 0, 100) / 100;

  const c = (1 - Math.abs(2 * normL - 1)) * normS;
  const x = c * (1 - Math.abs(((normH / 60) % 2) - 1));
  const m = normL - c / 2;
  let r = 0;
  let g = 0;
  let b = 0;

  if (normH < 60) {
    r = c;
    g = x;
    b = 0;
  } else if (normH < 120) {
    r = x;
    g = c;
    b = 0;
  } else if (normH < 180) {
    r = 0;
    g = c;
    b = x;
  } else if (normH < 240) {
    r = 0;
    g = x;
    b = c;
  } else if (normH < 300) {
    r = x;
    g = 0;
    b = c;
  } else {
    r = c;
    g = 0;
    b = x;
  }

  return {
    r: Math.round((r + m) * 255),
    g: Math.round((g + m) * 255),
    b: Math.round((b + m) * 255),
  };
}

/**
 * 将数值格式化为 16 进制字符串
 */
function toHexStr(r: number, g: number, b: number, a: number): string {
  const rHex = clamp(Math.round(r), 0, 255).toString(16).padStart(2, "0");
  const gHex = clamp(Math.round(g), 0, 255).toString(16).padStart(2, "0");
  const bHex = clamp(Math.round(b), 0, 255).toString(16).padStart(2, "0");
  if (a < 1) {
    const aHex = clamp(Math.round(a * 255), 0, 255).toString(16).padStart(2, "0");
    return `#${rHex}${gHex}${bHex}${aHex}`.toUpperCase();
  }
  return `#${rHex}${gHex}${bHex}`.toUpperCase();
}

/**
 * 将数值格式化为 RGB/RGBA 字符串
 */
function toRgbStr(r: number, g: number, b: number, a: number): string {
  if (a < 1) {
    const roundA = Math.round(a * 1000) / 1000;
    return `rgba(${r}, ${g}, ${b}, ${roundA})`;
  }
  return `rgb(${r}, ${g}, ${b})`;
}

/**
 * 将数值格式化为 HSL/HSLA 字符串
 */
function toHslStr(h: number, s: number, l: number, a: number): string {
  if (a < 1) {
    const roundA = Math.round(a * 1000) / 1000;
    return `hsla(${h}, ${s}%, ${l}%, ${roundA})`;
  }
  return `hsl(${h}, ${s}%, ${l}%)`;
}

/**
 * 智能嗅探字符串是否为色彩格式并解析为标准化色彩对象
 *
 * @param content 待检测文本
 * @return 匹配成功返回 ColorInfo，不匹配或非法返回 null
 */
export function parseColor(content: string): ColorInfo | null {
  if (!content) return null;
  const trimmed = content.trim();
  if (trimmed.length < 3 || trimmed.length > 50 || trimmed.includes("\n")) {
    return null;
  }

  // 1. HEX 格式匹配 (#RGB, #RGBA, #RRGGBB, #RRGGBBAA)
  const hexMatch = trimmed.match(/^#([0-9a-fA-F]{3,8})$/);
  if (hexMatch) {
    const hexDigits = hexMatch[1];
    let r = 0;
    let g = 0;
    let b = 0;
    let a = 1;

    if (hexDigits.length === 3) {
      r = parseInt(hexDigits[0] + hexDigits[0], 16);
      g = parseInt(hexDigits[1] + hexDigits[1], 16);
      b = parseInt(hexDigits[2] + hexDigits[2], 16);
    } else if (hexDigits.length === 4) {
      r = parseInt(hexDigits[0] + hexDigits[0], 16);
      g = parseInt(hexDigits[1] + hexDigits[1], 16);
      b = parseInt(hexDigits[2] + hexDigits[2], 16);
      a = parseInt(hexDigits[3] + hexDigits[3], 16) / 255;
    } else if (hexDigits.length === 6) {
      r = parseInt(hexDigits.slice(0, 2), 16);
      g = parseInt(hexDigits.slice(2, 4), 16);
      b = parseInt(hexDigits.slice(4, 6), 16);
    } else if (hexDigits.length === 8) {
      r = parseInt(hexDigits.slice(0, 2), 16);
      g = parseInt(hexDigits.slice(2, 4), 16);
      b = parseInt(hexDigits.slice(4, 6), 16);
      a = parseInt(hexDigits.slice(6, 8), 16) / 255;
    } else {
      return null;
    }

    const { h, s, l } = rgbToHsl(r, g, b);
    return {
      isValid: true,
      raw: trimmed,
      r,
      g,
      b,
      a,
      hex: toHexStr(r, g, b, a),
      rgb: toRgbStr(r, g, b, a),
      hsl: toHslStr(h, s, l, a),
    };
  }

  // 2. RGB / RGBA 格式匹配
  const rgbMatch = trimmed.match(
    /^rgba?\s*\(\s*(\d{1,3}%?)\s*[, ]\s*(\d{1,3}%?)\s*[, ]\s*(\d{1,3}%?)(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/i
  );
  if (rgbMatch) {
    const parseRgbVal = (val: string): number => {
      if (val.endsWith("%")) {
        return clamp(Math.round((parseFloat(val) / 100) * 255), 0, 255);
      }
      return clamp(Math.round(parseFloat(val)), 0, 255);
    };

    const r = parseRgbVal(rgbMatch[1]);
    const g = parseRgbVal(rgbMatch[2]);
    const b = parseRgbVal(rgbMatch[3]);
    let a = 1;

    if (rgbMatch[4] !== undefined) {
      const alphaRaw = rgbMatch[4];
      if (alphaRaw.endsWith("%")) {
        a = clamp(parseFloat(alphaRaw) / 100, 0, 1);
      } else {
        a = clamp(parseFloat(alphaRaw), 0, 1);
      }
    }

    const { h, s, l } = rgbToHsl(r, g, b);
    return {
      isValid: true,
      raw: trimmed,
      r,
      g,
      b,
      a,
      hex: toHexStr(r, g, b, a),
      rgb: toRgbStr(r, g, b, a),
      hsl: toHslStr(h, s, l, a),
    };
  }

  // 3. HSL / HSLA 格式匹配
  const hslMatch = trimmed.match(
    /^hsla?\s*\(\s*(\d{1,3}(?:deg)?)\s*[, ]\s*(\d{1,3}%)\s*[, ]\s*(\d{1,3}%)(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/i
  );
  if (hslMatch) {
    const h = ((parseFloat(hslMatch[1]) % 360) + 360) % 360;
    const s = clamp(parseFloat(hslMatch[2]), 0, 100);
    const l = clamp(parseFloat(hslMatch[3]), 0, 100);
    let a = 1;

    if (hslMatch[4] !== undefined) {
      const alphaRaw = hslMatch[4];
      if (alphaRaw.endsWith("%")) {
        a = clamp(parseFloat(alphaRaw) / 100, 0, 1);
      } else {
        a = clamp(parseFloat(alphaRaw), 0, 1);
      }
    }

    const { r, g, b } = hslToRgb(h, s, l);
    return {
      isValid: true,
      raw: trimmed,
      r,
      g,
      b,
      a,
      hex: toHexStr(r, g, b, a),
      rgb: toRgbStr(r, g, b, a),
      hsl: toHslStr(h, s, l, a),
    };
  }

  return null;
}
