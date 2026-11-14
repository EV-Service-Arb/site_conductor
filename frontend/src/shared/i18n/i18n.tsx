"use client";

import { type ReactNode, useEffect, useSyncExternalStore } from "react";

import type { Messages } from "@evinvest/i18n";
import { createI18nReact } from "@evinvest/i18n/react";

import ruAuto from "../../../messages/ru.auto.json";
import ru from "../../../messages/ru.json";

import { type Locale, i18n } from "./registry";

const { I18nProvider, useLocale, useT } = createI18nReact(i18n);
export { useLocale, useT };

const MESSAGES = { en: {}, ru: { ...ruAuto, ...ru } } satisfies Record<Locale, Messages>;

const CHOSEN = "sa:locale";
const CHOSEN_EVENT = "sa:locale-chosen";
const EVENTS = ["languagechange", "storage", CHOSEN_EVENT];

const subscribe = (onChange: () => void) => {
  for (const e of EVENTS) window.addEventListener(e, onChange);
  return () => {
    for (const e of EVENTS) window.removeEventListener(e, onChange);
  };
};

function current(): Locale {
  const chosen = localStorage.getItem(CHOSEN);
  if (chosen === null) return i18n.negotiate(navigator.language);
  if (!i18n.isLocale(chosen)) throw new Error(`${CHOSEN}: not a locale: ${chosen}`);
  return chosen;
}

export function chooseLocale(locale: Locale): void {
  localStorage.setItem(CHOSEN, locale);
  window.dispatchEvent(new Event(CHOSEN_EVENT));
}

/** The export is prerendered in English; the reader's choice, else the browser's language, takes over on hydration. */
export function I18n({ children }: { children: ReactNode }) {
  const locale = useSyncExternalStore(subscribe, current, () => i18n.defaultLocale);
  useEffect(() => {
    document.documentElement.lang = locale; // the export says "en"
  }, [locale]);
  return (
    <I18nProvider locale={locale} messages={MESSAGES[locale]} missing={process.env.NODE_ENV === "production" ? "throw" : "warn"}>
      {children}
    </I18nProvider>
  );
}
