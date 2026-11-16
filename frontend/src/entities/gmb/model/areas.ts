/** As `sa_core::ids::is_slug`, which the server holds an area's link to. */
export const isSlug = (s: string) => /^[a-z0-9][a-z0-9_-]{0,63}$/.test(s);

/** An area's link from its name: `Saint-Étienne` → `saint-etienne`. */
export const slugOf = (name: string) =>
  name
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
