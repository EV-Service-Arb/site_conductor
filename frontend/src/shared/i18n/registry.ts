import { createLocaleRegistry, type Translate } from "@evinvest/i18n";

export const i18n = createLocaleRegistry({
  locales: ["en", "ru"],
  labels: { en: "English", ru: "Русский" },
  default: "en",
});
export type Locale = (typeof i18n.locales)[number];
export type T = Translate;
