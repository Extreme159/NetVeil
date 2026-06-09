/**
 * NetVeil İçerik Boyutu (Content Scale) — %75 … %125, 5'erli adım.
 *
 * UI Scale'den (uiScale.ts) FARKI: pencere boyutunu/WebView zoom'unu DEĞİŞTİRMEZ.
 * Yalnızca içerik alanına (.main-scroll) CSS `zoom` uygular → font, satır boşlukları,
 * padding/gap (px dahil) hepsi ORANTILI ölçeklenir, pencere sabit kalır.
 *
 * Tercih localStorage'da saklanır, açılışta uygulanır.
 */

export type ContentScale = number; // 0.75 .. 1.25 (0.05 adım)

const STORAGE_KEY = "netveil_content_scale";

/** Geçerli değerler: %75, %80, ... %125 (11 seçenek). */
export const CONTENT_SCALE_OPTIONS: ContentScale[] = [
  0.75, 0.80, 0.85, 0.90, 0.95, 1.0, 1.05, 1.10, 1.15, 1.20, 1.25,
];

function clampToOption(v: number): ContentScale {
  // En yakın geçerli seçeneğe yuvarla (kayıtlı bozuk değere karşı güvenli).
  if (!Number.isFinite(v)) return 1.0;
  return CONTENT_SCALE_OPTIONS.reduce((best, opt) =>
    Math.abs(opt - v) < Math.abs(best - v) ? opt : best, 1.0);
}

function detectScale(): ContentScale {
  const raw = parseFloat(localStorage.getItem(STORAGE_KEY) ?? "");
  return Number.isFinite(raw) ? clampToOption(raw) : 1.0;
}

let currentScale: ContentScale = detectScale();
const listeners = new Set<() => void>();

export function getContentScale(): ContentScale {
  return currentScale;
}

/** CSS değişkenini (.main-scroll zoom kaynağı) uygular. */
export function applyContentScale(): void {
  document.documentElement.style.setProperty("--content-scale", String(currentScale));
}

export function setContentScale(scale: ContentScale): void {
  currentScale = clampToOption(scale);
  localStorage.setItem(STORAGE_KEY, String(currentScale));
  applyContentScale();
  listeners.forEach(fn => fn());
}

export function onContentScaleChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
