"use client";

import { Button } from "@evinvest/uikit";
import { Coins, Sparkles } from "lucide-react";
import { useState } from "react";

import { mail } from "@/entities/mail";
import { useTokens } from "@/entities/tokens";
import { ConnectModelDialog, useOwnModels } from "@/features/connect-model";
import { useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { useButtonSize } from "@/shared/ui/touch";

/** What gets mail that waits on the model read and answered again. */
export function Mend() {
  const t = useT();
  const button = useButtonSize();
  const mine = useOwnModels();
  const held = useTokens();
  const [connecting, setConnecting] = useState(false);
  if (mine === null) return null;
  return (
    <div className="flex flex-wrap gap-2">
      <Button variant="outline" size={button("sm")} onClick={() => setConnecting(true)}>
        <Sparkles aria-hidden />
        {mine.none ? t(mail.mend.connect, "Connect your own model") : t(mail.mend.reconnect, "Your own models")}
      </Button>
      {mine.none && held.status === "ok" && held.data.balance <= 0 && (
        <Button asChild variant="outline" size={button("sm")}>
          <ViewLink to={{ view: "tokens", params: {} }}>
            <Coins aria-hidden />
            {t(mail.mend.tokens, "Tokens: {n} left", { n: held.data.balance })}
          </ViewLink>
        </Button>
      )}
      <ConnectModelDialog open={connecting} onOpenChange={setConnecting} />
    </div>
  );
}
