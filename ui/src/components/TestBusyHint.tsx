import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";
import { useTestGuard } from "./testGuard";

/**
 * Baska bir test calisirken gosterilen kucuk ipucu (UI-01). `ownId` verilirse,
 * kilidi tutan test bu modulun kendisiyse ipucu gosterilmez.
 */
export function TestBusyHint({ ownId }: { ownId?: string }) {
  useLang();
  const { activeTest } = useTestGuard();
  if (activeTest === null || activeTest === ownId) return null;
  return (
    <div style={{ fontSize: 12, color: "#fbbf24", textAlign: "center", margin: "6px 0" }}>
      {t("test_busy_hint")}
    </div>
  );
}
