import { api, ok } from "@/shared/api";
import type { AlertHours, Destination } from "@/shared/api/schema";

import { startingHours } from "../model/context";

export const photoSrc = (id: number) => api.url("/alerts/inbox/{id}/photo", { path: { id } }).href;

export const readAlert = (id: number) => ok(api.POST("/alerts/inbox/read", { body: { ids: [id], up_to: id } }));

/** All of them, up to the newest shown: what came since was not seen. */
export const readAll = (newest: number) => ok(api.POST("/alerts/inbox/read", { body: { ids: null, up_to: newest } }));

export const pressAlert = (id: number, button: string) => ok(api.POST("/alerts/inbox/{id}/press", { params: { path: { id } }, body: { button } }));

export const setHours = (hours: AlertHours | null, change: Partial<AlertHours>) => ok(api.PUT("/alerts/hours", { body: { ...(hours ?? startingHours()), ...change } }));

export type Set = Partial<Pick<Destination, "title" | "cadences" | "rules" | "filter">>;

/** Changes some of a destination, the rest as it is. */
export const setDestination = (d: Destination, change: Set) =>
  ok(api.PUT("/alerts/destinations/{id}", { params: { path: { id: d.id } }, body: { title: d.title, cadences: d.cadences, rules: d.rules, filter: d.filter, ...change } }));
