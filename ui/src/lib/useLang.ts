import { useState, useEffect } from "react";
import { getLang, onLangChange, type Lang } from "./i18n";

/** Dil değişince re-render tetikleyen hook */
export function useLang(): Lang {
  const [lang, setLang] = useState<Lang>(getLang());
  useEffect(() => {
    return onLangChange(() => setLang(getLang()));
  }, []);
  return lang;
}
