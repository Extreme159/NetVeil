import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";

/**
 * Ortak sunucu seçici (Cloudflare / NetVeil.Net) — Speed Test ve Bufferbloat
 * modüllerinde paylaşılan GÖRSEL bileşen. Test mantığı/state her modülde özeldir;
 * burada yalnızca seçim UI'ı tek kaynaktan yönetilir.
 *
 * Değişiklik (logo/etiket/alt-metin/stil) yalnızca BU dosyada yapılır.
 */

export type ServerChoice = "cloudflare" | "netveil";

const SERVERS: { id: ServerChoice; label: string; sub: string; logo: string }[] = [
  { id: "cloudflare", label: "Cloudflare",  sub: "speed_srv_cloudflare_sub", logo: "/cloudflare-logo.svg" },
  { id: "netveil",    label: "NetVeil.Net", sub: "speed_srv_netveil_sub",    logo: "/netveil-mark.png" },
];

export function ServerPicker({ choice, setChoice, disabled }: {
  choice: ServerChoice;
  setChoice: (s: ServerChoice) => void;
  disabled?: boolean;
}) {
  useLang();
  return (
    <div style={{ display: "flex", gap: 8 }}>
      {SERVERS.map(s => (
        <button
          key={s.id}
          onClick={() => !disabled && setChoice(s.id)}
          disabled={disabled}
          style={{
            display: "flex", flexDirection: "column", alignItems: "flex-start",
            padding: "10px 14px", borderRadius: 10, border: "1px solid",
            cursor: disabled ? "not-allowed" : "pointer",
            background: choice === s.id ? "color-mix(in oklab, var(--accent) 12%, transparent)" : "rgba(255,255,255,0.03)",
            borderColor: choice === s.id ? "color-mix(in oklab, var(--accent) 40%, transparent)" : "rgba(255,255,255,0.08)",
            transition: "all 0.15s", minWidth: 140,
          }}
        >
          <span style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 13, fontWeight: 600, color: choice === s.id ? "var(--accent)" : "var(--text)" }}>
            <img src={s.logo} alt="" style={{ height: 14, width: "auto" }} />
            {s.label}
          </span>
          <span style={{ fontSize: 10, color: "var(--text-muted)", marginTop: 2 }}>{t(s.sub as Parameters<typeof t>[0])}</span>
        </button>
      ))}
    </div>
  );
}
