import { http } from "@/shared/api";
import type { Country, paths } from "@/shared/api/schema";

/** A photo's largest size, as the server takes it. */
export const PHOTO_MAX = 10 << 20;

const PHOTOS = "/trades/{id}/{country}/kit/photos" satisfies keyof paths;
const photos = (trade: number, country: Country) => `/api/v1${PHOTOS.replace("{id}", String(trade)).replace("{country}", country)}`;

/** Re-encoded by the server: a JPEG, ≤ 2048 px, no EXIF. Not through `api`: fetch tells nothing of an upload's progress. */
export function uploadPhoto(trade: number, country: Country, photo: Blob, onProgress: (sent: number) => void): Promise<void> {
  return http.upload("POST", photos(trade, country), photo, photo.type || "application/octet-stream", onProgress);
}

export const photoSrc = (trade: number, country: Country, photo: number) => `${photos(trade, country)}/${photo}.jpg`;
