import { api, ok } from "@/shared/api";
import type { MailCaseEdit, MailReadingEdit, MailSend, MailSettings, MailTopic } from "@/shared/api/schema";

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
export const sendDraft = (id: number, body: MailSend) => ok(api.POST("/mail/drafts/{id}/send", { ...path(id), body }));

/** What its case wants, given by a document of its GBP's or without one, or not given. */
export const giveMail = (id: number, item: string, given: boolean, document: number | null = null) => ok(api.PUT("/mail/cases/{id}/given", { ...path(id), body: { item, given, document } }));

/** That one alone, over what the others are now. */
export const setMailSetting = async (of: keyof MailSettings, to: number) => {
  const { set } = await ok(api.GET("/mail/settings"));
  return ok(api.PUT("/mail/settings", { body: { ...set, [of]: to } }));
};

export const switchAuto = (gmbs: number[], topic: MailTopic, on: boolean) => ok(api.PUT("/mail/auto", { body: { gmbs, topic, on } }));
