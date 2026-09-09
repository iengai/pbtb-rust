import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { messages, type Messages } from "./messages";

export type Lang = "en" | "zh";

const STORE_KEY = "pbtb.lang";

// The title is the brand, so it is the same in both languages; `<html lang>` is
// not, and screen readers and the CJK font fallbacks go by it.
const TITLE = "PBTB Console";

export function detectLang(): Lang {
  const first = navigator.languages?.[0] ?? navigator.language ?? "";
  return first.toLowerCase().startsWith("zh") ? "zh" : "en";
}

export function loadLang(): Lang {
  try {
    const saved = localStorage.getItem(STORE_KEY);
    if (saved === "en" || saved === "zh") return saved;
  } catch {
    // A browser with storage disabled still gets a language, just not a sticky one.
  }
  return detectLang();
}

export function saveLang(lang: Lang): void {
  try {
    localStorage.setItem(STORE_KEY, lang);
  } catch {
    // See loadLang.
  }
}

type Locale = { lang: Lang; setLang: (lang: Lang) => void; t: Messages };

const LocaleContext = createContext<Locale | null>(null);

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(loadLang);

  const setLang = useCallback((next: Lang) => {
    setLangState(next);
    saveLang(next);
  }, []);

  useEffect(() => {
    document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
    document.title = TITLE;
  }, [lang]);

  const value = useMemo<Locale>(() => ({ lang, setLang, t: messages[lang] }), [lang, setLang]);
  return <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>;
}

function useLocale(): Locale {
  const ctx = useContext(LocaleContext);
  if (!ctx) throw new Error("useLocale outside LocaleProvider");
  return ctx;
}

/** The catalog for the current language. */
export function useT(): Messages {
  return useLocale().t;
}

/** The current language, for the formatters that take one (dates, relative time). */
export function useLang(): { lang: Lang; setLang: (lang: Lang) => void } {
  const { lang, setLang } = useLocale();
  return { lang, setLang };
}

export function LangSwitch() {
  const { lang, setLang } = useLang();
  return (
    <div className="lang">
      <button
        type="button"
        className={lang === "en" ? "on" : ""}
        aria-pressed={lang === "en"}
        onClick={() => setLang("en")}
      >
        EN
      </button>
      <button
        type="button"
        className={lang === "zh" ? "on" : ""}
        aria-pressed={lang === "zh"}
        onClick={() => setLang("zh")}
      >
        中文
      </button>
    </div>
  );
}
