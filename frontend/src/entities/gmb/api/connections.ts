import { ApiError, http } from "@/shared/api";
import type { paths } from "@/shared/api/schema";

/** Every browser route's 503 while the server has no browser provider configured. */
export function browsersUnconnected(e: unknown): boolean {
  return e instanceof ApiError && e.failure.kind === "unavailable" && e.failure.message === "browsers not connected";
}

const FILM = "/gmails/{id}/browser/film" satisfies keyof paths;
const FILM_TYPE: keyof paths[typeof FILM]["put"]["requestBody"]["content"] = "video/mp4";

/** Played as the back camera by the browser's next launch; refused while it runs, or when ffmpeg cannot cut it. Not through `api`: fetch tells nothing of an upload's progress. */
export function uploadFilm(gmail: number, film: Blob, onProgress: (sent: number) => void): Promise<void> {
  return http.upload(`/api/v1${FILM.replace("{id}", String(gmail))}`, film, FILM_TYPE, onProgress);
}
