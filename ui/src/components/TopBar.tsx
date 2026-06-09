import type { ReactNode } from "react";
import { Icon } from "../lib/icons";
import type { TestPhase } from "../types";
import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";

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

  return (
    <div className="topbar">
      <div className="topbar-title">
        <h1>{title}</h1>
        <p>{subtitle}</p>
      </div>
      <div className="topbar-actions">
        {extraButtons}
        {buttonLabel !== null && buttonLabel !== undefined && (
          <button className="btn" onClick={onRun} disabled={running} type="button">
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
