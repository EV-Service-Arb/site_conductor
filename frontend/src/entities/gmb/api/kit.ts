import { api, ok } from "@/shared/api";
import openapi from "@/shared/api/openapi.json";
import type { Country } from "@/shared/api/schema";

const PHOTOS = openapi.paths["/trades/{id}/{country}/kit/photos"].post.requestBody.content;

/** A photo's largest size in bytes for its `type`, as the document's body has it; `undefined`: a type the server does not take. */
export const photoMax = (type: string): number | undefined => Object.entries(PHOTOS).find(([t]) => t === type)?.[1].schema.maxLength;

/** Re-encoded by the server: a JPEG, ≤ 2048 px, no EXIF. */
export async function uploadPhoto(trade: number, country: Country, photo: Blob, onProgress: (sent: number) => void): Promise<void> {
  await ok(api.upload("post", "/trades/{id}/{country}/kit/photos", { params: { path: { id: trade, country } }, body: photo, onProgress }));
}

export const photoSrc = (trade: number, country: Country, photo: number) => api.url("/trades/{id}/{country}/kit/photos/{file}", { path: { id: trade, country, file: `${photo}.jpg` } }).href;
