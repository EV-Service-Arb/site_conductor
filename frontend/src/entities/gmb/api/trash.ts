import { http, ignoreBody } from "@/shared/api";

import { type Trash, trashParser } from "../model/trash";

export function fetchTrash(): Promise<Trash> {
  return http.get("/api/v1/trash", trashParser);
}

/** With the GBPs removed with it. */
export async function restoreGmail(id: number): Promise<void> {
  await http.send("POST", `/api/v1/trash/gmails/${id}/restore`, undefined, ignoreBody);
}

export async function restoreGmb(id: number): Promise<void> {
  await http.send("POST", `/api/v1/trash/gmbs/${id}/restore`, undefined, ignoreBody);
}

/** `never_purge` absent: left as it is; present: `sa:admin:trash:keep` only. */
export async function setTrashSettings(keep_days: number, never_purge?: boolean): Promise<void> {
  await http.send("PUT", "/api/v1/trash/settings", never_purge === undefined ? { keep_days } : { keep_days, never_purge }, ignoreBody);
}
