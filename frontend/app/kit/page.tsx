import { KitView } from "@/views/kit";
import { PanelShell } from "@/views/shell";

/** `/<area>/` on the operator's phone (the server answers it with this page): signed in, without the panel's frame. */
export default function KitPage() {
  return (
    <PanelShell bare>
      <KitView />
    </PanelShell>
  );
}
