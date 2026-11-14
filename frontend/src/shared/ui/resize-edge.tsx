import { cn } from "@evinvest/uikit";
import { useState } from "react";

/** A column's `side` edge, dragged (or arrowed) to set its width; double-click puts back `initial`. Its parent is the column, positioned. */
export function ResizeEdge({
  side,
  width,
  bounds,
  onWidth,
  label,
}: {
  side: "left" | "right";
  width: number;
  bounds: { min: number; max: number; initial: number };
  onWidth: (width: number) => void;
  label: string;
}) {
  const [dragging, setDragging] = useState(false);
  const outward = side === "left" ? "ArrowLeft" : "ArrowRight";
  const inward = side === "left" ? "ArrowRight" : "ArrowLeft";
  return (
    <>
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={label}
        aria-valuemin={bounds.min}
        aria-valuemax={bounds.max}
        aria-valuenow={width}
        tabIndex={0}
        onKeyDown={(e) => {
          const step = e.key === outward ? 24 : e.key === inward ? -24 : 0;
          if (step === 0) return;
          e.preventDefault();
          onWidth(width + step);
        }}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          setDragging(true);
        }}
        onPointerMove={(e) => {
          if (!dragging) return;
          const column = e.currentTarget.parentElement!.getBoundingClientRect();
          onWidth(side === "left" ? column.right - e.clientX : e.clientX - column.left);
        }}
        onPointerUp={() => setDragging(false)}
        onDoubleClick={() => onWidth(bounds.initial)}
        className={cn(
          "absolute inset-y-0 z-10 w-2 cursor-col-resize outline-none after:absolute after:inset-y-0 after:left-0.75 after:w-0.5 after:transition-colors hover:after:bg-border-strong focus-visible:after:bg-ring",
          side === "left" ? "-left-1" : "-right-1",
          dragging && "after:bg-ring",
        )}
      />
      {dragging && <div className="fixed inset-0 z-50 cursor-col-resize" />}
    </>
  );
}
