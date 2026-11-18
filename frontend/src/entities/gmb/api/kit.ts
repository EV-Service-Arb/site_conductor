import { api, ok } from "@/shared/api";
import openapi from "@/shared/api/openapi.json";

const PHOTOS = openapi.paths["/brands/{id}/photos"].post.requestBody.content;

/** A photo's largest size in bytes for its `type`, as the document's body has it; `undefined`: a type the server does not take. */
export const photoMax = (type: string): number | undefined => Object.entries(PHOTOS).find(([t]) => t === type)?.[1].schema.maxLength;

/** Re-encoded by the server: a JPEG, ≤ 2048 px, no EXIF. */
export async function uploadPhoto(brand: number, photo: Blob, onProgress: (sent: number) => void): Promise<void> {
  await ok(api.upload("post", "/brands/{id}/photos", { params: { path: { id: brand } }, body: photo, onProgress }));
}

export const photoSrc = (brand: number, photo: number) => api.url("/brands/{id}/photos/{file}", { path: { id: brand, file: `${photo}.jpg` } }).href;

/** Re-encoded by the server: a PNG, ≤ 512 px. */
export async function uploadLogo(brand: number, logo: Blob): Promise<void> {
  await ok(api.upload("put", "/brands/{id}/logo", { params: { path: { id: brand } }, body: logo, onProgress: () => {} }));
}

/** `version`: the brand's `logo`, so a new one is fetched anew. */
export const logoSrc = (brand: number, version: string) => `${api.url("/brands/{id}/logo", { path: { id: brand } }).href}?v=${version}`;
