"use client";

import { Button, Input, cn } from "@evinvest/uikit";
import { Crosshair, GripVertical, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { type Candidate, type Country, type Spot, setOnMap, useOnMap } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize, useControlSize } from "@/shared/ui/touch";

import { add as addKeys } from "../model/i18n";
import { PlaceSearch } from "./place-search";

export const spotKey = (s: { lat: number; lng: number }) => `${s.lat},${s.lng}`;

/**
 * Spots in order, the first a draft's point: added off Google's suggestions or picked on the map; `single` holds one, without a note
 * (a GBP past a draft). `onChange` gets the country Google gave a spot it just added.
 */
export function SpotsEditor({
  spots,
  gmail,
  single,
  onChange,
}: {
  spots: readonly Spot[];
  gmail: number | null;
  single?: boolean;
  onChange: (spots: Spot[], added?: { spot: Spot; country: Country | null }) => void;
}) {
  const t = useT();
  const button = useButtonSize();
  const picking = useOnMap((s) => s.picking !== null);
  const [dragged, setDragged] = useState<number | null>(null);
  // the map's click answers after this render: it adds to the spots as they are then
  const latest = useRef({ spots, onChange });
  useEffect(() => {
    latest.current = { spots, onChange };
  });

  const added = (spot: Spot, country: Country | null) => {
    const { spots, onChange } = latest.current;
    if (spots.some((s) => spotKey(s) === spotKey(spot))) {
      notifyFailure(new Error(t(addKeys.spots.twice, "That spot is listed already.")), t);
      return;
    }
    onChange([...spots, spot], { spot, country });
  };
  const fromGoogle = (c: Candidate | null) => {
    if (c === null) return;
    if (c.location === null) {
      notifyFailure(new Error(t(addKeys.spots.noPoint, "Google gives {place} no point: pick it on the map.", { place: c.name ?? c.address ?? c.place_id })), t);
      return;
    }
    added({ ...c.location, label: c.name ?? c.address, note: null }, c.country);
  };
  const moved = (from: number, to: number) => {
    if (to < 0 || to >= spots.length || from === to) return;
    const next = [...spots];
    next.splice(to, 0, ...next.splice(from, 1));
    onChange(next);
  };
  const full = single === true && spots.length > 0;

  return (
    <div className="flex flex-col gap-2">
      {spots.length > 0 && (
        <ol className="flex flex-col divide-y divide-border rounded-md border border-border">
          {spots.map((s, i) => (
            <li
              key={spotKey(s)}
              draggable={!single}
              onDragStart={() => setDragged(i)}
              onDragOver={(e) => {
                if (dragged === null) return;
                e.preventDefault();
                if (dragged !== i) {
                  moved(dragged, i);
                  setDragged(i);
                }
              }}
              onDragEnd={() => setDragged(null)}
              className={cn("flex items-center gap-2 px-2 py-2", dragged === i && "opacity-50")}
            >
              {!single && (
                <button
                  type="button"
                  aria-label={t(addKeys.spots.move, "Move spot {n}: ↑ ↓", { n: i + 1 })}
                  title={t(addKeys.spots.drag, "Drag to reorder")}
                  className="cursor-grab text-ink-faint hover:text-ink-soft"
                  onKeyDown={(e) => {
                    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
                    e.preventDefault();
                    moved(i, e.key === "ArrowUp" ? i - 1 : i + 1);
                  }}
                >
                  <GripVertical aria-hidden className="size-4" />
                </button>
              )}
              {!single && (
                <span className={cn("flex size-5 shrink-0 items-center justify-center rounded-full text-xs font-semibold", i === 0 ? "bg-primary text-primary-foreground" : "bg-muted text-ink")}>
                  {i + 1}
                </span>
              )}
              <span className="flex min-w-0 flex-1 flex-col gap-1">
                <span className={cn("truncate text-sm", s.label === null ? "text-ink-soft" : "text-ink")}>{s.label ?? t(addKeys.spots.point, "A point on the map")}</span>
                <span className="truncate font-mono text-xs text-ink-faint">
                  {s.lat.toFixed(4)}, {s.lng.toFixed(4)}
                </span>
                {!single && <Note value={s.note} onSave={(note) => onChange(spots.map((x, j) => (j === i ? { ...x, note } : x)))} />}
              </span>
              <Button
                type="button"
                variant="ghost"
                size={button("sm")}
                icon
                aria-label={t(addKeys.spots.remove, "Remove spot {n}", { n: i + 1 })}
                title={t(addKeys.spots.remove, "Remove spot {n}", { n: i + 1 })}
                onClick={() => onChange(spots.filter((_, j) => j !== i))}
              >
                <X aria-hidden />
              </Button>
            </li>
          ))}
        </ol>
      )}
      {!full && (
        <div className="flex items-start gap-2">
          <div className="min-w-0 flex-1">
            <PlaceSearch
              gmail={gmail}
              placeholder={spots.length === 0 ? t(addKeys.where.placeholder, "An address, a street or a business nearby") : t(addKeys.spots.another, "Another spot: an address, a business nearby")}
              picked={null}
              onPick={fromGoogle}
            />
          </div>
          <Button
            type="button"
            variant={picking ? "primary" : "outline"}
            size={button()}
            icon
            aria-pressed={picking}
            aria-label={t(addKeys.spots.pick, "Pick it on the map")}
            title={t(addKeys.spots.pick, "Pick it on the map")}
            onClick={() =>
              setOnMap({
                picking: picking
                  ? null
                  : {
                      label: t(addKeys.spots.picking, "Click the map where spot {n} stands", { n: latest.current.spots.length + 1 }),
                      onPick: (at) => added({ ...at, label: null, note: null }, null),
                    },
              })
            }
          >
            <Crosshair aria-hidden />
          </Button>
        </div>
      )}
    </div>
  );
}

/** Saved as it is left. */
function Note({ value, onSave }: { value: string | null; onSave: (note: string | null) => void }) {
  const t = useT();
  const size = useControlSize();
  const [typed, setTyped] = useState(value ?? "");
  const save = () => {
    const note = typed.trim() === "" ? null : typed.trim();
    if (note !== value) onSave(note);
  };
  return (
    <Input
      size={size}
      value={typed}
      placeholder={t(addKeys.spots.note, "A note")}
      aria-label={t(addKeys.spots.note, "A note")}
      onChange={(e) => setTyped(e.target.value)}
      onBlur={save}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          save();
        }
      }}
    />
  );
}
