// NetVeil portable build script.
//
// Tauri release binary'sini üretir, dist/ klasörüne versiyon adıyla kopyalar.
// Çıktı: dist/NetVeil-v0.3.0-portable.exe
//
// Kullanım: npm run build:portable
//
// Notlar:
//  - WebView2 Windows 10/11'de built-in, ek runtime gerekmez
//  - .exe self-contained: tüm asset'ler binary içinde (frontendDist)
//  - PowerShell admin gerektirmez, ama ICMP ping için yine admin/UAC gerekir
//  - İlk build 3-10 dk sürer (LTO + release optimizasyonu)
import { execSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const DIST = join(ROOT, "dist");

const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
const version = pkg.version;

const isWindows = process.platform === "win32";
const exeName = isWindows ? "netveil.exe" : "netveil";
const sourcePath = join(ROOT, "src-tauri", "target", "release", exeName);

console.log(`\n🔨 NetVeil v${version} portable build başlıyor...\n`);
console.log(`   Platform: ${process.platform}`);
console.log(`   Hedef:    ${sourcePath}\n`);

// 1. Frontend build (Vite)
console.log("📦 [1/3] Frontend build (Vite)...");
execSync("npm run build", { cwd: ROOT, stdio: "inherit" });

// 2. Tauri release build — SADECE binary, installer/MSI yok
console.log("\n🦀 [2/3] Tauri release build (cargo + LTO, ~3-10 dk)...");
// --no-bundle: MSI/NSIS installer üretme, sadece .exe
execSync("npx tauri build --no-bundle", { cwd: ROOT, stdio: "inherit" });

// 3. Portable .exe'yi dist/ altına kopyala
console.log("\n📋 [3/3] Portable .exe kopyalanıyor...");
if (!existsSync(sourcePath)) {
  console.error(`❌ Binary bulunamadı: ${sourcePath}`);
  process.exit(1);
}

if (!existsSync(DIST)) mkdirSync(DIST, { recursive: true });

const outName = isWindows
  ? `NetVeil-v${version}-portable.exe`
  : `NetVeil-v${version}-portable`;
const outPath = join(DIST, outName);
copyFileSync(sourcePath, outPath);

const sizeMB = (statSync(outPath).size / 1024 / 1024).toFixed(2);
console.log(`\n✅ Portable build tamam!\n`);
console.log(`   📁 Dosya: ${outPath}`);
console.log(`   📏 Boyut: ${sizeMB} MB`);
console.log(`   🚀 Çift tıkla, kuruluma gerek yok.\n`);
console.log(`   Not: ICMP ping için "Yönetici olarak çalıştır" gerekir.\n`);
