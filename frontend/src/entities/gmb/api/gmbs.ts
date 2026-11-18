import type { UndoEntry } from "@evinvest/uikit";

import { api, ok } from "@/shared/api";
import type { Gmb, LatLng, Point } from "@/shared/api/schema";
import type { T } from "@/shared/i18n";

import { undo } from "../model/i18n";

/** What a write sets on each GBP, and how it writes a value; `of` reads an absent field as the `null` it means. */
interface Field<V> {
  of: (gmb: Gmb) => V;
  write: (ids: number[], value: V) => Promise<void>;
}

const LOCATION: Field<Point | null> = {
  of: (b) => b.location,
  write: async (ids, at) => {
    for (const id of ids) {
      const path = { params: { path: { id } } };
      if (at === null) await ok(api.DELETE("/gmbs/{id}/location", path));
      else await ok(api.PUT("/gmbs/{id}/location", { ...path, body: { lat: at.lat, lng: at.lng } }));
    }
  },
};
const ENABLED: Field<boolean> = {
  of: (b) => b.enabled,
  write: async (gmbs, enabled) => {
    await ok(api.PUT("/gmbs/enabled", { body: { gmbs, enabled } }));
  },
};
const TRADE: Field<number | null> = {
  of: (b) => b.trade_id,
  write: async (gmbs, trade) => {
    await ok(api.PUT("/gmbs/trade", { body: { gmbs, trade } }));
  },
};

/**
 * `gmbs`, as the client held them, given `to`. The entry writes either side back only while every
 * one still holds the other, as the conductor answers at the press (another operator, another tab).
 */
async function set<V>(field: Field<V>, gmbs: Gmb[], to: V, label: string, t: T): Promise<UndoEntry> {
  const ids = gmbs.map((b) => b.id);
  await field.write(ids, to);
  const was = new Map(gmbs.map((b) => [b.id, JSON.stringify(field.of(b))]));
  const after = JSON.stringify(to);
  const holds = async (value: (id: number) => string) => {
    const now = new Map((await ok(api.GET("/gmails"))).gmbs.map((b) => [b.id, JSON.stringify(field.of(b))]));
    if (!ids.every((id) => now.get(id) === value(id))) throw new Error(t(undo.changed, "Not undone: {label} has changed since. The history is cleared.", { label }));
  };
  return {
    label,
    undo: async () => {
      await holds(() => after);
      const back = new Map<string, number[]>();
      for (const [id, v] of was) back.set(v, [...(back.get(v) ?? []), id]);
      for (const [v, group] of back) await field.write(group, JSON.parse(v) as V);
    },
    redo: async () => {
      await holds((id) => was.get(id)!);
      await field.write(ids, to);
    },
  };
}

/** Refused when Google has the place's point: only one it has none of is placed by hand. */
export function placeGmb(gmb: Gmb, at: LatLng, t: T): Promise<UndoEntry> {
  return set(LOCATION, [gmb], { lat: at.lat, lng: at.lng, placed: true }, t(undo.placed, "{name} placed on the map", { name: gmb.name }), t);
}

/** A verified GBP overwritten with its profile on Google, asked afresh and paid by the caller. */
export function syncGmb(gmb: Gmb): Promise<Gmb> {
  return ok(api.POST("/gmbs/{id}/sync", { params: { path: { id: gmb.id } } }));
}

export const gmbPhotoSrc = (gmb: number, photo: number) => api.url("/gmbs/{id}/photos/{photo}", { path: { id: gmb, photo } }).href;

/** All of `gmbs` or none: one not the caller's refuses the lot. */
export function switchGmbs(gmbs: Gmb[], enabled: boolean, t: T): Promise<UndoEntry> {
  return set(ENABLED, gmbs, enabled, (enabled ? t(undo.on, "{n} switched on", { n: gmbs.length }) : t(undo.off, "{n} switched off", { n: gmbs.length })), t);
}

/** All of `gmbs` or none: one not the caller's refuses the lot. `null` takes their trade away. */
export function setTrades(gmbs: Gmb[], trade: number | null, t: T): Promise<UndoEntry> {
  return set(TRADE, gmbs, trade, t(undo.trade, "Trade set on {n}", { n: gmbs.length }), t);
}
