"use client";

import { useResource } from "@evinvest/api/react";
import { Button, Spinner } from "@evinvest/uikit";
import { Paperclip } from "lucide-react";
import type { Ref } from "react";

import { api } from "@/shared/api";
import { useT } from "@/shared/i18n";
import { useButtonSize } from "@/shared/ui/touch";

import { gmbs as gmbsKeys } from "../model/i18n";

/** What is kept for a GBP, newest first. */
export function useDocuments(gmb: number) {
  return useResource(api, "/gmbs/{id}/documents", { params: { path: { id: gmb } } });
}

export const fileSize = (bytes: number) => (bytes < 100 * 1024 ? `${Math.ceil(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`);

/** Files picked off the device: on a phone, its camera's too. */
export function AttachFiles({ multiple = false, busy = false, input, onFiles }: { multiple?: boolean; busy?: boolean; input?: Ref<HTMLInputElement>; onFiles: (files: File[]) => void }) {
  const t = useT();
  const button = useButtonSize();
  return (
    <Button asChild variant="outline" size={button("sm")}>
      <label className="relative cursor-pointer has-focus-visible:ring-2 has-focus-visible:ring-ring has-disabled:opacity-50">
        {busy ? <Spinner aria-hidden /> : <Paperclip aria-hidden />}
        {multiple ? t(gmbsKeys.documents.attach, "Attach files") : t(gmbsKeys.documents.attach.one, "Attach")}
        <input
          ref={input}
          type="file"
          multiple={multiple}
          disabled={busy}
          className="sr-only"
          onChange={(e) => {
            const picked = [...(e.target.files ?? [])];
            e.target.value = "";
            if (picked.length > 0) onFiles(picked);
          }}
        />
      </label>
    </Button>
  );
}
