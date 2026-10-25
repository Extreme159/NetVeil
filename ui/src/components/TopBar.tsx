import type { ReactNode } from "react";
import { Icon } from "../lib/icons";
import type { TestPhase } from "../types";
import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";
import { useTestGuard } from "./testGuard";

interface Props {
  title: string;
  subtitle: string;
  phase: TestPhase;
  onRun: () => void;
  buttonLabel?: string | null;
  /** Dashboard gibi sekmelerde TopBar sağına ek butonlar yerleştirmek için */
  extraButtons?: ReactNode;
}

export function TopBar({ title, subtitle, phase, onRun, buttonLabel, extraButtons }: Props) {
  useLang();
  const running = phase.startsWith("running_");
  // Baska bir test (yerel-state'li sekmeler dahil) calisiyorsa yeni test baslatilamaz (UI-01).
  const { activeTest } = useTestGuard();
  const blocked = activeTest !== null;

  return (
    <div className="topbar">
      <div className="topbar-title">
        <h1>{title}</h1>
        <p>{subtitle}</p>
      </div>
      <div className="topbar-actions">
        {extraButtons}
        {blocked && !running && buttonLabel !== null && buttonLabel !== undefined && (
          <span style={{ fontSize: 12, color: "#fbbf24" }}>{t("test_busy_hint")}</span>
        )}
        {buttonLabel !== null && buttonLabel !== undefined && (
          <button className="btn" onClick={onRun} disabled={running || blocked} title={blocked && !running ? t("test_busy_hint") : undefined} type="button">
            {running ? (
              <Icon.Refresh style={{ animation: "spin 1s linear infinite" }} />
            ) : (
              <Icon.Play />
            )}
            {running ? t("btn_running") : (buttonLabel ?? t("btn_run_full"))}
          </button>
        )}
      </div>
    </div>
  );
}
