/** "aquafix, vifnet" → ["aquafix", "vifnet"]; null if none is given. Each is the server's to refuse: no document gives a brand's form. */
export function brandsFrom(raw: string): string[] | null {
  const brands = [...new Set(raw.split(/[\s,]+/).filter(Boolean))];
  return brands.length > 0 ? brands : null;
}
