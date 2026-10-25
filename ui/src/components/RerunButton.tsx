import { Icon } from "../lib/icons";
import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";
import { useTestGuard } from "./testGuard";

/**
 * Tamamlanmis tani gorunumlerinde 'Tekrar calistir' (UI-03). Calisan test varsa
 * (kendisi dahil) pasif; baska bir test calisiyorsa ipucu gosterir.
 */
export function RerunButton({ onRun, running }: { onRun: () => void; running: boolean }) {
  useLang();
  const { activeTest } = useTestGuard();
  const blocked = activeTest !== null;
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 10 }}>
      {blocked && !running && (
        <span style={{ fontSize: 12, color: "#fbbf24" }}>{t("test_busy_hint")}</span>
      )}
      <button className="btn btn-secondary" type="button" onClick={onRun} disabled={running || blocked}>
        {running
          ? <Icon.Refresh style={{ animation: "spin 1s linear infinite" }} />
          : <Icon.Play />}
        {running ? t("btn_running") : t("btn_run_again")}
      </button>
    </div>
  );
}
