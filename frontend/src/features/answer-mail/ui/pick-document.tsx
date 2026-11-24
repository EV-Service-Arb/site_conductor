"use client";

import { Button, DropdownMenu, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@evinvest/uikit";
import { FolderOpen } from "lucide-react";
import { useState } from "react";

import { AttachFiles, gmbPhotoSrc, keepDocument, useDocuments } from "@/entities/gmb";
import { mail } from "@/entities/mail";
import type { Gmb, GmbDocument } from "@/shared/api/schema";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";

/** A document of the GBP's: files kept with it at once, as what `givenFor` asks, one kept before, or a photo Google shows of it. */
export function PickDocument({ gmb, documents, givenFor, onPicked }: { gmb: Gmb; documents: ReturnType<typeof useDocuments>; givenFor: string | null; onPicked: (picked: readonly GmbDocument[]) => void | Promise<void> }) {
  const t = useT();
  const button = useButtonSize();
  const [busy, setBusy] = useState(false);
  const pick = async (from: () => Promise<readonly GmbDocument[]>) => {
    setBusy(true);
    try {
      await onPicked(await from());
    } catch (e) {
      notifyFailure(e, t);
    } finally {
      setBusy(false);
      documents.reload();
    }
  };
  const keep = (file: File) => keepDocument(gmb.id, file, givenFor);
  const kept = documents.status === "ok" ? documents.data.documents : [];
  const photo = async (id: number) => {
    const name = `photo-${id}.jpg`;
    const had = kept.find((d) => d.name === name);
    if (had !== undefined) return [had];
    const answer = await fetch(gmbPhotoSrc(gmb.id, id));
    if (!answer.ok) throw new Error(t(mail.pick.photo.failed, "The photo did not load: {status}", { status: answer.status }));
    return [await keep(new File([await answer.blob()], name, { type: "image/jpeg" }))];
  };
  return (
    <>
      <AttachFiles multiple={givenFor === null} busy={busy} onFiles={(files) => void pick(() => Promise.all(files.map(keep)))} />
      {kept.length + gmb.photos.length > 0 && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size={button("sm")} disabled={busy}>
              <FolderOpen aria-hidden />
              {t(mail.pick.kept, "Kept")}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="max-h-80 w-72 overflow-y-auto">
            {kept.length > 0 && (
              <DropdownMenuGroup>
                <DropdownMenuLabel>{t(mail.pick.documents, "Its documents")}</DropdownMenuLabel>
                {kept.map((d) => (
                  <DropdownMenuItem key={d.id} onClick={() => void pick(() => Promise.resolve([d]))}>
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate">{d.name}</span>
                      {d.given_for !== null && <span className="truncate text-xs text-ink-soft">{d.given_for}</span>}
                    </span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuGroup>
            )}
            {gmb.photos.length > 0 && (
              <DropdownMenuGroup>
                <DropdownMenuLabel>{t(mail.pick.photos, "Its photos on Google")}</DropdownMenuLabel>
                {gmb.photos.map((p) => (
                  <DropdownMenuItem key={p.id} onClick={() => void pick(() => photo(p.id))}>
                    {/* eslint-disable-next-line @next/next/no-img-element -- the API's own JPEG, behind the session: nothing for next/image to optimise in an export */}
                    <img src={gmbPhotoSrc(gmb.id, p.id)} alt="" loading="lazy" className="h-10 w-14 shrink-0 rounded-sm object-cover" />
                    <span className="truncate">{p.author ?? t(mail.pick.photo, "Photo {id}", { id: p.id })}</span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuGroup>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </>
  );
}
