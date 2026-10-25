// Her edit/release sonrası: `npm run bump` ile versiyon arttır.
// Tek yerden — 5 dosyayı + 2 lock kök sürümünü senkron tutar (package.json, ui/package.json,
// src-tauri/tauri.conf.json, src-tauri/Cargo.toml, README.md).
// Sunucu deploy'u bu script'ten YAPILMAZ — `--deploy` reddedilir.
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
//   npm run bump major        → 0.x.x → 1.0.10 (script davranışı; final 1.0.0 ayrı karar)
//   npm run bump -- --no-deploy → geri uyum için kabul edilir (no-op)
//   npm run bump -- --deploy  → REDDEDİLİR (exit 1, hiçbir dosya yazılmaz)
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);

// `--deploy` (tek başına veya `--no-deploy` ile birlikte) HER DOSYA YAZIMINDAN ÖNCE reddedilir.
if (args.includes("--deploy")) {
  console.error(
    "\n❌ --deploy devre dışı: bump sunucu deploy etmez.\n" +
    "   Hiçbir dosya değiştirilmedi.\n"
  );
  process.exit(1);
}
// fix (default) = basit düzeltme; feature/minor = önemli özellik. "patch" da fix sayılır (geri uyum).
const rawKind = args.find(a => ["fix","patch","feature","minor","major"].includes(a)) || "fix";
const KIND = rawKind === "patch" ? "fix" : (rawKind === "minor" ? "feature" : rawKind);
// `--no-deploy` geri uyum için kabul edilir (no-op); deploy yolu yoktur.

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

// npm lock dosyalarında yalnız kök paket sürümü (üst "version" + packages[""].version) eşitlenir;
// bağımlılık girdilerine dokunulmaz.
function updateLock(path, version) {
  const content = readFileSync(path, "utf8");
  const top = /^(  "version"\s*:\s*")[^"]+(")/m;
  const root = /("packages"\s*:\s*\{\s*""\s*:\s*\{[^}]*?"version"\s*:\s*")[^"]+(")/;
  if (!top.test(content) || !root.test(content)) {
    console.error(`\n❌ ${path.replace(ROOT, ".")}: kök paket sürüm alanları bulunamadı.\n`);
    process.exit(1);
  }
  const swap = (_m, pre, post) => `${pre}${version}${post}`;
  writeFileSync(path, content.replace(top, swap).replace(root, swap));
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
updateLock(join(ROOT, "package-lock.json"), newVersion);
updateLock(join(ROOT, "ui/package-lock.json"), newVersion);
updateJson(join(ROOT, "src-tauri/tauri.conf.json"), newVersion);
updateCargo(join(ROOT, "src-tauri/Cargo.toml"), newVersion);
// Not: Versiyon UI'da artik Settings'te gosterilir, kaynak ui/package.json (yukarida
// updateJson ile guncellenir). Sidebar versiyon gostermez (Sidebar regex'i kaldirildi).
updateReadme(join(ROOT, "README.md"), newVersion);

console.log(`\n✨ Bitti. Yeni versiyon: ${newVersion}\n`);
