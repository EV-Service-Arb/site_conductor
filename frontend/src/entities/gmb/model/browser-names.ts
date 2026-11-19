import type { Device, Provider } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";

import { browser as browserKeys } from "./i18n";

export const PROVIDER_NAME = {
  gologin: (t) => t(browserKeys.provider.gologin, "GoLogin"),
  multilogin: (t) => t(browserKeys.provider.multilogin, "Multilogin · cloud phone"),
  multilogin_browser: (t) => t(browserKeys.provider.multilogin_browser, "Multilogin · browser"),
} satisfies Record<Provider, (t: T) => string>;

export const DEVICE_NAME = {
  galaxy_s23: (t) => t(browserKeys.device.galaxy_s23, "Galaxy S23"),
  pixel_7: (t) => t(browserKeys.device.pixel_7, "Pixel 7"),
  redmi_note_14: (t) => t(browserKeys.device.redmi_note_14, "Redmi Note 14"),
} satisfies Record<Device, (t: T) => string>;
