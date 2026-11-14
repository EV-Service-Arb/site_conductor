"use client";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
  Button,
  buttonVariants,
  toast,
} from "@evinvest/uikit";

import { revokeSource, sources } from "@/entities/source";
import { useT } from "@/shared/i18n";
import { notifyFailure } from "@/shared/ui/notify";
import { useButtonSize } from "@/shared/ui/touch";
import { move } from "@/entities/gmb";

export function RevokeButton({ keyId, onRevoked }: { keyId: string; onRevoked: () => void }) {
  const t = useT();
  const button = useButtonSize();
  const revoke = async () => {
    try {
      await revokeSource(keyId);
      toast.positive(t(sources.revoked.toast, "Source revoked"));
      onRevoked();
    } catch (e) {
      notifyFailure(e, t);
    }
  };
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="ghost" size={button("sm")}>
          {t(sources.revoke, "Revoke")}
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t(sources.revoke.title, "Revoke {key}?", { key: keyId })}</AlertDialogTitle>
          <AlertDialogDescription>{t(sources.revoke.body, "Events signed with this key will be refused from now on. This cannot be undone; a new key has to be added instead.")}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel className={buttonVariants({ variant: "outline", size: button() })}>{t(move.cancel, "Cancel")}</AlertDialogCancel>
          <AlertDialogAction className={buttonVariants({ variant: "destructive", size: button() })} onClick={() => void revoke()}>
            {t(sources.revoke.confirm, "Revoke")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
