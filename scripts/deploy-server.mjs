// netveil.net sunucusuna manuel deploy.
// Kullanım: npm run deploy:server
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execSync } from "node:child_process";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const serverMainPath = join(ROOT, "server/src/main.rs");

const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
console.log(`\n🚀 netveil.net deploy başlıyor (v${pkg.version})...\n`);

try {
  console.log("  📤 main.rs kopyalanıyor...");
  execSync(`scp "${serverMainPath}" root@netveil.net:/opt/netveil-server/src/main.rs`, { stdio: "pipe" });

  console.log("  🔨 Sunucuda build yapılıyor...");
  execSync(
    `ssh root@netveil.net "cd /opt/netveil-server && /root/.cargo/bin/cargo build --release --quiet"`,
    { stdio: "pipe" }
  );

  console.log("  🔄 Servis yeniden başlatılıyor...");
  execSync(`ssh root@netveil.net "systemctl restart netveil-server"`, { stdio: "pipe" });

  // Doğrulama
  const result = execSync(`ssh root@netveil.net "curl -s https://netveil.net/version"`, { encoding: "utf8" });
  const remote = JSON.parse(result.trim());
  console.log(`\n✅ Deploy tamamlandı — netveil.net/version: ${remote.version}\n`);
} catch (err) {
  console.error(`\n❌ Deploy hatası: ${err.message}\n`);
  process.exit(1);
}
