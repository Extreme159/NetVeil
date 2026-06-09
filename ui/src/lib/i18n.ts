/**
 * NetVeil i18n — TR / EN
 * Çeviriler: ui/src/lib/translations.json
 * Kullanım:  import { t } from "../lib/i18n"
 */

import dict from "./translations.json";

export type Lang = "en" | "tr";

const STORAGE_KEY = "netveil_lang";

/** OS diline göre başlangıç dilini belirle */
function detectLang(): Lang {
  const saved = localStorage.getItem(STORAGE_KEY) as Lang | null;
  if (saved === "tr" || saved === "en") return saved;
  const sys = navigator.language?.toLowerCase() ?? "";
  return sys.startsWith("tr") ? "tr" : "en";
}

// Reaktif dil state'i — React dışında basit event sistemi
let currentLang: Lang = detectLang();
const listeners = new Set<() => void>();

export function getLang(): Lang { return currentLang; }

export function setLang(lang: Lang) {
  currentLang = lang;
  localStorage.setItem(STORAGE_KEY, lang);
  listeners.forEach(fn => fn());
}

export function onLangChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

type Key = keyof typeof dict;

/** Çeviri fonksiyonu */
export function t(key: Key): string {
  const entry = dict[key] as { en: string; tr: string } | undefined;
  if (!entry) return key;
  return entry[currentLang] ?? entry.en;
}
