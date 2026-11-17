import { ok, sites } from "@/shared/api";

/** sites' place answer is not read: the page re-reads its tags. */
export async function linkPlace(brand: string, slug: string, gmbId: number | null): Promise<void> {
  await ok(sites.PUT("/places/{brand}/{slug}/gmb", { params: { path: { brand, slug } }, body: { gmb_id: gmbId } }));
}
