"use client";

import { Button, DropdownMenuItem } from "@evinvest/uikit";
import { MapPinPlus, Move } from "lucide-react";
import { usePathname, useRouter } from "next/navigation";

import { type Gmb, peek, setOnMap, gmbs } from "@/entities/gmb";
import { useT } from "@/shared/i18n";
import { useButtonSize } from "@/shared/ui/touch";

/** Opens the home map to place a GMB Google has no point for, or to move the one placed: nothing for a point of Google's. */
export function PlaceButton({ gmb, item = false, onNavigate }: { gmb: Gmb; item?: boolean; onNavigate?: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const router = useRouter();
  const pathname = usePathname();
  if (gmb.location !== null && !gmb.location.placed) return null;
  const label = (gmb.location === null ? t(gmbs.place.set, "Set location") : t(gmbs.place.move, "Move location"));
  const open = () => {
    if (pathname !== "/") router.push("/");
    else peek(null);
    setOnMap({ placing: gmb.id });
    onNavigate?.();
  };
  const icon = gmb.location === null ? <MapPinPlus aria-hidden /> : <Move aria-hidden />;
  if (item)
    return (
      <DropdownMenuItem onClick={open}>
        {icon}
        {label}
      </DropdownMenuItem>
    );
  return (
    <Button variant="ghost" size={button("sm")} icon aria-label={label} title={label} onClick={open}>
      {icon}
    </Button>
  );
}
