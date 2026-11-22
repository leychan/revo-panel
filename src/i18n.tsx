import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { english } from "./messages.en";

export type Language = "zh-CN" | "en";
export type LanguagePreference = "system" | Language;
type Values = Record<string, string | number | Message | null | undefined>;
type Message = { key: string; values?: Values };
export type Localized = string | Message;
export type Translate = (value: Localized | null | undefined, values?: Values) => string;

const preferenceKey = "revo-panel.language.v1";
const slot = /\{(\w+)\}/g;

export function message(key: string, values?: Values): Message {
  return { key, values };
}

function initialPreference(): LanguagePreference {
  try {
    const value = localStorage.getItem(preferenceKey);
    if (value === "en" || value === "zh-CN") return value;
  } catch { /* 存储不可用时仍然使用系统语言。 */ }
  return "system";
}

function systemLanguage(): Language {
  const supported = [navigator.language, ...navigator.languages].find((locale) => /^(zh|en)(-|$)/i.test(locale));
  return supported?.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

// 原生助手原有的诊断消息仍可供 CLI 使用，界面按已知模板翻译。
const patterns = Object.keys(english).filter((key) => key.includes("{")).map((key) => {
  const names: string[] = [];
  const expression = key.split(/(\{\w+\})/).map((part) => {
    if (part.startsWith("{")) {
      names.push(part.slice(1, -1));
      return "(.+?)";
    }
    return Array.from(part, (char) => "\\^$.*+?()[]{}|".includes(char) ? "\\" + char : char).join("");
  }).join("");
  return { key, names, expression: new RegExp("^" + expression + "$", "s") };
});

export function translate(language: Language): Translate {
  const t: Translate = (value, values) => {
    if (value == null) return "";
    let key = typeof value === "string" ? value : value.key;
    let args = typeof value === "string" ? values : value.values;
    if (language === "en" && !args && /\p{Script=Han}/u.test(key) && !Object.hasOwn(english, key)) {
      for (const pattern of patterns) {
        const found = pattern.expression.exec(key);
        if (found) {
          args = Object.fromEntries(pattern.names.map((name, index) => [name, t(found[index + 1])]));
          key = pattern.key;
          break;
        }
      }
    }
    const text = language === "en" && Object.hasOwn(english, key) ? english[key] : key;
    if (!args) return text;
    return text.replace(slot, (_, name: string) => {
      const item = args[name];
      return item == null ? "—" : typeof item === "object" ? t(item) : String(item);
    });
  };
  return t;
}

const Locale = createContext({ language: "en" as Language, preference: "system" as LanguagePreference,
  setPreference: (_: LanguagePreference) => {}, t: translate("en") });

export function I18nProvider({ children }: { children: ReactNode }) {
  const [preference, setPreference] = useState(initialPreference);
  const [system, setSystem] = useState(systemLanguage);
  const language = preference === "system" ? system : preference;
  const t = useMemo(() => translate(language), [language]);
  const trayUpdate = useRef(Promise.resolve());

  useEffect(() => {
    let active = true;
    const update = () => setSystem(systemLanguage());
    if (isTauri()) {
      void invoke<Language>("system_language").then((value) => {
        if (active) setSystem(value);
      }).catch(console.error);
    }
    window.addEventListener("languagechange", update);
    return () => { active = false; window.removeEventListener("languagechange", update); };
  }, []);

  useLayoutEffect(() => { document.documentElement.lang = language; }, [language]);

  useEffect(() => {
    try { localStorage.setItem(preferenceKey, preference); } catch { /* 当前窗口仍可切换。 */ }
  }, [preference]);

  useEffect(() => {
    if (isTauri()) {
      // 依次通知托盘，快速切换时最后一次选择保持生效。
      trayUpdate.current = trayUpdate.current.then(() => invoke<void>("set_language", { language })).catch(console.error);
    }
  }, [language]);

  return <Locale.Provider value={{ language, preference, setPreference, t }}>{children}</Locale.Provider>;
}

export const useI18n = () => useContext(Locale);
