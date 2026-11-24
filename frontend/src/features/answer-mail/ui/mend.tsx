"use client";

import { Button } from "@evinvest/uikit";
import { Coins, Sparkles } from "lucide-react";
import { useState } from "react";

import { mail } from "@/entities/mail";
import { useTokens } from "@/entities/tokens";
import { ConnectClaudeDialog, useClaude } from "@/features/connect-claude";
import { useT } from "@/shared/i18n";
import { ViewLink } from "@/shared/lib/location";
import { useButtonSize } from "@/shared/ui/touch";

/** What gets mail that waits on the model read and answered again. */
export function Mend() {
  const t = useT();
  const button = useButtonSize();
  const mine = useClaude();
  const held = useTokens();
  const [connecting, setConnecting] = useState(false);
  if (mine.status !== "ok") return null;
  return (
    <div className="flex flex-wrap gap-2">
      <Button variant="outline" size={button("sm")} onClick={() => setConnecting(true)}>
        <Sparkles aria-hidden />
        {mine.data === null ? t(mail.mend.connect, "Connect your Claude") : t(mail.mend.reconnect, "Reconnect your Claude")}
      </Button>
      {mine.data === null && held.status === "ok" && held.data.balance <= 0 && (
        <Button asChild variant="outline" size={button("sm")}>
          <ViewLink to={{ view: "tokens", params: {} }}>
            <Coins aria-hidden />
            {t(mail.mend.tokens, "Tokens: {n} left", { n: held.data.balance })}
          </ViewLink>
        </Button>
      )}
      <ConnectClaudeDialog open={connecting} onOpenChange={setConnecting} />
    </div>
  );
}
