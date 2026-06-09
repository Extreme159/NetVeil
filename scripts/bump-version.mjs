// Her edit/release sonrası: `npm run bump` ile versiyon arttır.
// Tek yerden — 6 dosyayı senkron tutar + netveil.net sunucusunu günceller.
//
// BETA VERSİYON ŞEMASI (v1.0.0'a kadar): v0.MINOR.PATCH — PATCH HEP 10-99 ARASI.
// KRİTİK: patch ASLA 00-09 OLMAZ — leading-zero (örn 0.7.01) cargo semver'i KIRAR.
// Bu yüzden patch 10'dan başlar, hep 2 haneli ama ≥10.
//   Basit düzeltme (fix)      → patch +1            (0.7.10 → 0.7.11)
//   Önemli özellik/modül      → minor +1, patch=10   (0.7.x → 0.8.10)
//   Patch 99'da taşma         → minor +1, patch=10   (0.7.99 → 0.8.10)
//   Patch <10 (eski/geçiş)    → fix 10'a sıçrar      (0.7.1 → 0.7.10)
//
// Kullanım:
//   npm run bump              → fix: patch +1           (0.7.10 → 0.7.11)
//   npm run bump feature      → özellik: minor +1, patch=10 (0.7.x → 0.8.10)
//   npm run bump major        → 0.x.x → 1.0.0
//   npm run bump --no-deploy  → sunucu güncellemesini atla
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execSync } from "node:child_process";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
// fix (default) = basit düzeltme; feature/minor = önemli özellik. "patch" da fix sayılır (geri uyum).
const rawKind = args.find(a => ["fix","patch","feature","minor","major"].includes(a)) || "fix";
const KIND = rawKind === "patch" ? "fix" : (rawKind === "minor" ? "feature" : rawKind);
const NO_DEPLOY = args.includes("--no-deploy");

// Beta şeması: patch HEP 10-99 (leading-zero YOK — cargo semver kuralı).
// fix = patch+1 (99'da minor+1 & patch=10), feature = minor+1 & patch=10,
// patch<10 (eski/geçiş sürüm) fix yapılınca direkt 10'a sıçrar. major = klasik.
const PATCH_START = 10;

function bump(version, kind) {
  const [major, minor, patch] = version.split(".").map(Number);
  if (kind === "major") return `${major + 1}.0.${PATCH_START}`;
  if (kind === "feature") return `${major}.${minor + 1}.${PATCH_START}`;
  // fix: patch+1, ama <10 ise (geçiş sürüm) PATCH_START'a sıçra, 99'u aşarsa minor taşması.
  if (patch < PATCH_START) return `${major}.${minor}.${PATCH_START}`;
  if (patch >= 99) return `${major}.${minor + 1}.${PATCH_START}`;
  return `${major}.${minor}.${patch + 1}`;
}

function updateJson(path, version) {
  const content = readFileSync(path, "utf8");
  const updated = content.replace(/"version"\s*:\s*"[^"]+"/, `"version": "${version}"`);
  writeFileSync(path, updated);
  console.log(`  ✓ ${path.replace(ROOT, ".")}`);
}

function updateCargo(path, version) {
  const content = readFileSync(path, "utf8");
  const updated = content.replace(/^version\s*=\s*"[^"]+"/m, `version = "${version}"`);
  writeFileSync(path, updated);
  console.log(`  ✓ ${path.replace(ROOT, ".")}`);
}

function updateReadme(path, version) {
  const content = readFileSync(path, "utf8");
  const updated = content.replace(
    /!\[Version\]\(https:\/\/img\.shields\.io\/badge\/version-[^-]+-purple\)/,
    `![Version](https://img.shields.io/badge/version-${version}-purple)`
  );
  writeFileSync(path, updated);
  console.log(`  ✓ ${path.replace(ROOT, ".")}`);
}

// Mevcut versiyonu package.json'dan oku
const pkgPath = join(ROOT, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
const oldVersion = pkg.version;
const newVersion = bump(oldVersion, KIND);

console.log(`\n📦 Versiyon: ${oldVersion} → ${newVersion} (${KIND})\n`);

updateJson(pkgPath, newVersion);
updateJson(join(ROOT, "ui/package.json"), newVersion);
updateJson(join(ROOT, "src-tauri/tauri.conf.json"), newVersion);
updateCargo(join(ROOT, "src-tauri/Cargo.toml"), newVersion);
// Not: Versiyon UI'da artik Settings'te gosterilir, kaynak ui/package.json (yukarida
// updateJson ile guncellenir). Sidebar versiyon gostermez (Sidebar regex'i kaldirildi).
updateReadme(join(ROOT, "README.md"), newVersion);

// server/src/main.rs içindeki /version endpoint'ini güncelle
function updateServerVersion(path, version) {
  const content = readFileSync(path, "utf8");
  const updated = content.replace(
    /\{\\"version\\":\\"[^\\"]+\\"/,
    `{\\"version\\":\\"${version}\\"`
  );
  if (content === updated) {
    console.log(`  ⚠ ${path.replace(ROOT, ".")} — version satırı bulunamadı, atlandı`);
    return;
  }
  writeFileSync(path, updated);
  console.log(`  ✓ ${path.replace(ROOT, ".")}`);
}

const serverMainPath = join(ROOT, "server/src/main.rs");
updateServerVersion(serverMainPath, newVersion);

console.log(`\n✨ Bitti. Yeni versiyon: ${newVersion}\n`);

// Sunucu deploy — SSH üzerinden otomatik
if (NO_DEPLOY) {
  console.log("⏩ Sunucu deploy atlandı (--no-deploy).\n");
} else {
  console.log("🚀 netveil.net sunucusu güncelleniyor...");
  try {
    execSync(
      `scp "${serverMainPath}" root@netveil.net:/opt/netveil-server/src/main.rs`,
      { stdio: "pipe" }
    );
    execSync(
      `ssh root@netveil.net "cd /opt/netveil-server && /root/.cargo/bin/cargo build --release --quiet && systemctl restart netveil-server"`,
      { stdio: "pipe" }
    );
    // Deploy sonrasi dogrulama
    const result = execSync("ssh root@netveil.net \"curl -s https://netveil.net/version\"", { encoding: "utf8" });
    const remote = JSON.parse(result.trim());
    if (remote.version === newVersion) {
      console.log(`  ✓ netveil.net/version → ${remote.version} ✔\n`);
    } else {
      console.log(`  ⚠ netveil.net/version → ${remote.version} (beklenen: ${newVersion})\n`);
    }
  } catch (err) {
    console.error(`  ✗ Sunucu deploy hatası: ${err.message}`);
    console.error("    Manuel deploy: npm run deploy:server\n");
  }
}
