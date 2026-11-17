import { api, ok } from "@/shared/api";
import type { Country } from "@/shared/api/schema";

/** A photo's largest size, as the server takes it. */
export const PHOTO_MAX = 10 << 20;

/** Re-encoded by the server: a JPEG, ≤ 2048 px, no EXIF. */
export async function uploadPhoto(trade: number, country: Country, photo: Blob, onProgress: (sent: number) => void): Promise<void> {
  await ok(api.upload("post", "/trades/{id}/{country}/kit/photos", { params: { path: { id: trade, country } }, body: photo, onProgress }));
}

export const photoSrc = (trade: number, country: Country, photo: number) => api.url("/trades/{id}/{country}/kit/photos/{file}", { path: { id: trade, country, file: `${photo}.jpg` } }).href;
