import { api, ok } from "@/shared/api";
import type { MailCaseEdit, MailFile, MailReadingEdit, MailTopic } from "@/shared/api/schema";

const path = (id: number) => ({ params: { path: { id } } });

export const editCase = (id: number, body: MailCaseEdit) => ok(api.PATCH("/mail/cases/{id}", { ...path(id), body }));

export const correctMail = (id: number, body: MailReadingEdit) => ok(api.PATCH("/mail/messages/{id}", { ...path(id), body }));

/** Written anew in place of the one there. */
export const writeDraft = (id: number, instruction: string | null) => ok(api.POST("/mail/cases/{id}/draft", { ...path(id), body: { instruction } }));

/** One of the operator's own, empty. */
export const beginDraft = (id: number) => ok(api.POST("/mail/cases/{id}/draft", { ...path(id), body: { body: "" } }));

/** One going by itself waits for its operator after. */
export const keepDraft = (id: number, body: string) => ok(api.PUT("/mail/drafts/{id}", { ...path(id), body: { body } }));

export const dismissDraft = (id: number) => ok(api.DELETE("/mail/drafts/{id}", path(id)));

/** A dismissed one. */
export const restoreDraft = (id: number) => ok(api.POST("/mail/drafts/{id}/restore", path(id)));

/** To Google, once; `as_is`: with no file, whatever it lacks. */
export const sendDraft = (id: number, body: string, files: MailFile[], as_is: boolean) => ok(api.POST("/mail/drafts/{id}/send", { ...path(id), body: { body, files, as_is } }));

export const switchAuto = (gmail: number, topic: MailTopic, on: boolean) => ok(api.PUT("/mail/auto", { body: { gmail, topic, on } }));
