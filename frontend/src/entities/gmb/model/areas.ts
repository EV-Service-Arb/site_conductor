import openapi from "@/shared/api/openapi.json";

const slug = openapi.paths["/areas/{slug}"].put.parameters.find((p) => p.name === "slug");
if (slug === undefined) throw new Error("the document's PUT /areas/{slug} has no slug");
const SLUG = new RegExp(slug.schema.pattern);

/** An area's link as the document's `{slug}` takes it. */
export const isSlug = (s: string) => SLUG.test(s);

/** An area's link from its name: `Saint-Étienne` → `saint-etienne`. */
export const slugOf = (name: string) =>
  name
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
