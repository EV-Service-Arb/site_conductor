import { api, ok } from "@/shared/api";
import type { GmbDocument, MailFile } from "@/shared/api/schema";

export async function encoded(file: File): Promise<MailFile> {
  const url = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
  // a kind the browser does not know is bytes, as RFC 2046 has it
  return { name: file.name, type: file.type === "" ? "application/octet-stream" : file.type, base64: url.slice(url.indexOf(",") + 1) };
}

/** `file` kept for the GBP, as what `given_for` asks. */
export async function keepDocument(gmb: number, file: File, given_for: string | null): Promise<GmbDocument> {
  const { documents } = await ok(api.POST("/gmbs/{id}/documents", { params: { path: { id: gmb } }, body: { file: await encoded(file), given_for } }));
  const [kept] = documents.filter((d) => d.name === file.name && d.size === file.size).sort((a, b) => b.id - a.id);
  if (kept === undefined) throw new Error(`${file.name} was kept, and is not among GBP ${gmb}'s documents`);
  return kept;
}

export const removeDocument = (gmb: number, document: number) => ok(api.DELETE("/gmbs/{id}/documents/{document}", { params: { path: { id: gmb, document } } }));

/** Handed to the browser as a download. */
export async function saveDocument(gmb: number, document: number): Promise<void> {
  const f = await ok(api.GET("/gmbs/{id}/documents/{document}", { params: { path: { id: gmb, document } } }));
  const href = URL.createObjectURL(new Blob([Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0))], { type: f.type }));
  Object.assign(window.document.createElement("a"), { href, download: f.name }).click();
  URL.revokeObjectURL(href);
}
