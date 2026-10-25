export interface PresetTarget {
  label: string;
  host: string;
  ip_support?: "4" | "6" | "both";
}

function parseIpv4(host: string): number[] | null {
  const parts = host.split(".");
  if (parts.length !== 4 || parts.some(p => !/^(0|[1-9][0-9]{0,2})$/.test(p) || Number(p) > 255)) return null;
  return parts.map(Number);
}

// One policy (IANA special-purpose address registries), duplicated verbatim in
// src-tauri/src/remote_list.rs and server/src/main.rs: change all three together.
function isPublicIpv4([a, b, c, d]: number[]): boolean {
  return !(a === 0 || a === 10 || a === 127 || a >= 224
    || (a === 100 && b >= 64 && b <= 127)                      // CGNAT 100.64/10
    || (a === 169 && b === 254)                                // link-local
    || (a === 172 && b >= 16 && b <= 31)                       // 172.16/12
    || (a === 192 && b === 0 && c === 0 && d !== 9 && d !== 10) // 192.0.0.0/24 except 192.0.0.9/.10
    || (a === 192 && b === 0 && c === 2)                       // TEST-NET-1
    || (a === 192 && b === 88 && c === 99)                     // 6to4 relay (deprecated)
    || (a === 192 && b === 168)
    || (a === 198 && (b === 18 || b === 19))                   // benchmarking 198.18/15
    || (a === 198 && b === 51 && c === 100)                    // TEST-NET-2
    || (a === 203 && b === 0 && c === 113));                   // TEST-NET-3
}

function isPublicIpv6(host: string): boolean {
  // The platform URL parser validates IPv6 syntax and normalizes dotted IPv4 tails.
  // Restrict the input first: no brackets, zone IDs, ports, paths or URL escapes.
  if (!/^[0-9a-f:.]+$/i.test(host)) return false;
  let normalized: string;
  try {
    normalized = new URL(`http://[${host}]/`).hostname.slice(1, -1);
  } catch {
    return false;
  }
  const [left, right] = normalized.split("::");
  const head = left ? left.split(":") : [];
  const tail = right ? right.split(":") : [];
  const segments = right === undefined ? head : [...head, ...Array(8 - head.length - tail.length).fill("0"), ...tail];
  const s = segments.map(p => parseInt(p, 16));
  const embedded = (hi: number, lo: number) => isPublicIpv4([hi >> 8, hi & 255, lo >> 8, lo & 255]);

  // Keep parity with src-tauri/src/remote_list.rs: mapped/NAT64/6to4 are judged by their embedded IPv4.
  if (s.slice(0, 5).every(n => n === 0) && s[5] === 0xffff) return embedded(s[6], s[7]); // ::ffff:0:0/96
  if (s[0] === 0x64 && s[1] === 0xff9b && s.slice(2, 6).every(n => n === 0)) return embedded(s[6], s[7]); // 64:ff9b::/96
  // ::/8 is IETF-reserved (::/128, ::1, IPv4-compatible, 64:ff9b:1::/48 all fall in here).
  if ((s[0] & 0xff00) === 0) return false;
  if (s[0] === 0x2002) return embedded(s[1], s[2]); // 2002::/16
  // 2001::/23 minus the globally reachable 2001:1::1/128, 2001:1::2/128, 2001:3::/32,
  // 2001:4:112::/48, 2001:20::/28, 2001:30::/28.
  const in2001 = s[0] === 0x2001 && (s[1] & 0xfe00) === 0;
  const in2001Global = (s[1] === 1 && s.slice(2, 7).every(n => n === 0) && (s[7] === 1 || s[7] === 2))
    || s[1] === 3
    || (s[1] === 4 && s[2] === 0x112)
    || (s[1] & 0xfff0) === 0x20
    || (s[1] & 0xfff0) === 0x30;
  return !((in2001 && !in2001Global)
    || (s[0] === 0x100 && s[1] === 0 && s[2] === 0 && s[3] <= 1)    // 100::/64 + 100:0:0:1::/64 (RFC 9780)
    || (s[0] === 0x2001 && s[1] === 0x0db8)                         // 2001:db8::/32
    || (s[0] === 0x3fff && (s[1] & 0xf000) === 0)                   // 3fff::/20
    || s[0] === 0x5f00                                              // 5f00::/16
    || (s[0] & 0xfe00) === 0xfc00                                   // fc00::/7
    || (s[0] & 0xffc0) === 0xfe80                                   // fe80::/10
    || (s[0] & 0xffc0) === 0xfec0                                   // fec0::/10
    || (s[0] & 0xff00) === 0xff00);                                 // ff00::/8
}

/** Syntax/literal-IP filtering only; DNS resolution/rebinding remains a backend concern. */
export function isValidTargetHost(host: string): boolean {
  if (!host || host.length > 253 || /[^a-zA-Z0-9.:-]/.test(host)) return false;
  if (host.includes(":")) return isPublicIpv6(host);
  const ipv4 = parseIpv4(host);
  if (ipv4) return isPublicIpv4(ipv4);

  const lower = host.toLowerCase();
  const localSuffixes = [".localhost", ".local", ".localdomain", ".internal", ".lan", ".intranet", ".home.arpa"];
  if (localSuffixes.some(suffix => lower.endsWith(suffix))) return false;
  const labels = lower.split(".");
  return labels.length >= 2
    && labels.every(label => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label))
    // WHATWG "ends in a number": all-digit or 0x-hex last label is parsed as IPv4 (0x7f.1, 127.1, 2130706433).
    && !/^(?:[0-9]+|0x[0-9a-f]*)$/.test(labels[labels.length - 1]);
}

/** Validate each JSON entry independently, preserving valid entries in a mixed list. */
export function filterRemotePresets(data: unknown): PresetTarget[] {
  if (!Array.isArray(data)) return [];
  return data.filter((entry): entry is PresetTarget => {
    if (typeof entry !== "object" || entry === null) return false;
    return typeof entry.label === "string" && entry.label.trim().length > 0
      && typeof entry.host === "string" && isValidTargetHost(entry.host)
      && (entry.ip_support === undefined || entry.ip_support === "4" || entry.ip_support === "6" || entry.ip_support === "both");
  });
}
