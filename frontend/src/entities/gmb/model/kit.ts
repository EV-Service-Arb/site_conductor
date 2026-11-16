import type { KitTarget } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";

import { kit } from "./i18n";

export const TARGET_NAME = {
  google: (t) => t(kit.target.google, "Google's form"),
  drafts: (t) => t(kit.target.drafts, "Drafts page"),
  one_tap: (t) => t(kit.target.oneTap, "One tap"),
} satisfies Record<KitTarget, (t: T) => string>;
