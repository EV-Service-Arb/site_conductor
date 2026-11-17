import { Refused, api, ok } from "@/shared/api";

/** Every browser route's 503 while the server has no browser provider configured. */
export function browsersUnconnected(e: unknown): boolean {
  return e instanceof Refused && e.failure.code === "browsers_not_connected";
}

/** Played as the back camera by the browser's next launch; refused while it runs, or when ffmpeg cannot cut it. */
export async function uploadFilm(gmail: number, film: Blob, onProgress: (sent: number) => void): Promise<void> {
  await ok(api.upload("put", "/gmails/{id}/browser/film", { params: { path: { id: gmail } }, body: film, onProgress }));
}
