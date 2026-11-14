"use client";

import { createLocaleRegistry, type Translate } from "@evinvest/i18n";
import { createPolicy } from "@evinvest/i18n/policy";
import { createI18nReact } from "@evinvest/i18n/react";
import { type ReactNode, useEffect, useSyncExternalStore } from "react";

import en from "../../../messages/en/common.json";
import ruOverrides from "../../../messages/ru/common.json";
import ruAuto from "../../../messages/ru/common.auto.json";

/** Each language by its own name. */
export const LOCALES = { en: "English", ru: "Русский" } as const;
export type Locale = keyof typeof LOCALES;
export type T = Translate;

const i18n = createLocaleRegistry({ locales: ["en", "ru"], labels: LOCALES, default: "en" });
const { I18nProvider, useT } = createI18nReact(i18n);
export { useT };

const MESSAGES = { en, ru: createPolicy(i18n).resolveCatalogue("ru", en, ruAuto, ruOverrides).messages };

const CHOSEN = "sa:locale";
const CHOSEN_EVENT = "sa:locale-chosen";
const EVENTS = ["languagechange", "storage", CHOSEN_EVENT];

const subscribe = (onChange: () => void) => {
  for (const e of EVENTS) window.addEventListener(e, onChange);
  return () => {
    for (const e of EVENTS) window.removeEventListener(e, onChange);
  };
};

/** The panel speaks the reader's browser language when it is Russian, English otherwise. */
function current(): Locale {
  const chosen = localStorage.getItem(CHOSEN);
  if (chosen === null) return navigator.language.toLowerCase().startsWith("ru") ? "ru" : "en";
  if (!(chosen in LOCALES)) throw new Error(`${CHOSEN}: not a locale: ${chosen}`);
  return chosen as Locale;
}

/** The export is prerendered in English; the reader's choice, else the browser's language, takes over on hydration. */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribe, current, () => "en");
}

export function chooseLocale(locale: Locale): void {
  localStorage.setItem(CHOSEN, locale);
  window.dispatchEvent(new Event(CHOSEN_EVENT));
}

/** Translates the tree in the reader's language and keeps `<html lang>` on it (the export says "en"). */
export function LocaleProvider({ children }: { children: ReactNode }) {
  const locale = useLocale();
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);
  return (
    <I18nProvider locale={locale} messages={MESSAGES[locale]} missing="warn">
      {children}
    </I18nProvider>
  );
}
