/**
 * NetVeil UI Scale — %75 / %100 / %125
 * WebView zoom factor + pencere boyutu birlikte ayarlanir (Yorum B).
 * Tercih localStorage'da saklanir, acilista uygulanir (i18n pattern'i).
 */

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

export type UiScale = 0.75 | 1.0 | 1.25;

const STORAGE_KEY = "netveil_ui_scale";

// Baz tasarim boyutu (tauri.conf.json ile ayni)
const BASE_W = 1280;
const BASE_H = 900;

const VALID: UiScale[] = [0.75, 1.0, 1.25];

function detectScale(): UiScale {
  const saved = parseFloat(localStorage.getItem(STORAGE_KEY) ?? "");
  return VALID.includes(saved as UiScale) ? (saved as UiScale) : 1.0;
}

let currentScale: UiScale = detectScale();
const listeners = new Set<() => void>();

export function getScale(): UiScale {
  return currentScale;
}

/** Zoom + pencere boyutunu uygular (Tauri JS API). */
export async function applyScale(): Promise<void> {
  try {
    await getCurrentWebview().setZoom(currentScale);
    await getCurrentWindow().setSize(
      new LogicalSize(BASE_W * currentScale, BASE_H * currentScale),
    );
  } catch (e) {
    // Tarayicida (tauri yokken) veya izin sorununda sessizce gec.
    console.error("[uiScale] uygulanamadi:", e);
  }
}

export function setScale(scale: UiScale): void {
  currentScale = scale;
  localStorage.setItem(STORAGE_KEY, String(scale));
  void applyScale();
  listeners.forEach(fn => fn());
}

export function onScaleChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
