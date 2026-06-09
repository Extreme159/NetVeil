//! Yerel kalıcı veri katmanı (SQLite / rusqlite bundled).
//!
//! Tasarım notları:
//! - DB dosyası Tauri app_data_dir altında `netveil.db`.
//! - WAL modu: arka plan monitör yazarken UI okuyabilsin (tek-yazar/çok-okuyucu).
//! - Şema versiyonu `PRAGMA user_version` ile takip edilir; `run_migrations()`
//!   ileri uyumlu migration adımlarını sırayla uygular.
//! - Connection paylaşımı YOK: yazımlar nadir (outage olayları) ve her biri
//!   kısa-ömürlü bir bağlantıyla yapılır. Bu, async runtime'ı bloklamamak için
//!   çağıran taraftan `spawn_blocking` içinde çalıştırılmalıdır.
//!
//! İleriki adımlar (roadmap, STATUS.md):
//! - Adım 2: `test_runs` tablosu + geçmiş karşılaştırma sorguları + Tauri komutları.
//! - Adım 3: `uploaded` flag tabanlı sunucu senkron kuyruğu.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Mevcut şema versiyonu. Yeni migration eklerken artır ve `run_migrations`'a ekle.
const SCHEMA_VERSION: i64 = 3;

/// Bir epoch-ms zaman damgasından YEREL saate göre gün etiketi üretir ('2026-05-29').
/// Gün sınırı kullanıcının yerel saatine göredir (kullanıcı-merkezli "o günkü veri").
pub fn day_label(ts_ms: u64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ts_ms as i64).single() {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => "1970-01-01".to_string(),
    }
}

/// AppHandle'dan DB dosya yolunu çözer (app_data_dir/netveil.db). Dizini oluşturur.
pub fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir alınamadı: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("veri dizini oluşturulamadı: {e}"))?;
    Ok(dir.join("netveil.db"))
}

/// Bağlantı açar, pragmaları ayarlar ve migration'ları uygular.
/// Her çağrı kısa-ömürlü bir bağlantı döndürür (paylaşımlı global state yok).
pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("db açılamadı: {e}"))?;
    // WAL: çok-okuyucu/tek-yazar; NORMAL: WAL ile güvenli ve hızlı.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("WAL ayarlanamadı: {e}"))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| format!("synchronous ayarlanamadı: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("busy_timeout ayarlanamadı: {e}"))?;
    run_migrations(&conn)?;
    Ok(conn)
}

/// `user_version`'a göre eksik migration adımlarını sırayla uygular.
fn run_migrations(conn: &Connection) -> Result<(), String> {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| format!("user_version okunamadı: {e}"))?;

    if current < 1 {
        // v1: ağ kararlılık kopma olayları
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS outage_events (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                ts           INTEGER NOT NULL,           -- olay zamanı (epoch ms)
                kind         TEXT    NOT NULL,           -- 'gateway' | 'wan' | 'full'
                phase        TEXT    NOT NULL,           -- 'start' | 'end'
                duration_ms  INTEGER,                    -- yalnızca 'end' fazında dolu
                created_at   INTEGER NOT NULL            -- kaydın yazıldığı an (epoch ms)
            );
            CREATE INDEX IF NOT EXISTS idx_outage_ts ON outage_events(ts);",
        )
        .map_err(|e| format!("v1 migration başarısız: {e}"))?;
    }

    if current < 2 {
        // v2: gün-merkezli kararlılık telemetrisi
        //   - daily_stability : gün başına TEK satır (hızlı sorgu için ön-toplanmış gerçek)
        //   - stability_buckets: dakikalık RTT özeti (trend grafiği)
        //   - outage_events'e day kolonu (gün bazlı sorgu için)
        // NOT: Yalnızca SAYISAL GERÇEK saklanır; skor/uyarı/yorum YOK (okuma anında üretilir).
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS daily_stability (
                day               TEXT    PRIMARY KEY,   -- 'YYYY-MM-DD' (yerel saat)
                monitored_ms      INTEGER NOT NULL DEFAULT 0, -- o gün toplam izlenen süre (ms)
                sample_count      INTEGER NOT NULL DEFAULT 0, -- toplam ölçüm örneği
                fail_count        INTEGER NOT NULL DEFAULT 0, -- başarısız (timeout) örnek sayısı
                outage_count      INTEGER NOT NULL DEFAULT 0, -- biten kopma olayı sayısı
                total_outage_ms   INTEGER NOT NULL DEFAULT 0, -- toplam kopma süresi (ms)
                gw_rtt_sum        REAL    NOT NULL DEFAULT 0, -- ort hesabı için (sum/count)
                gw_rtt_n          INTEGER NOT NULL DEFAULT 0,
                wan_rtt_sum       REAL    NOT NULL DEFAULT 0,
                wan_rtt_n         INTEGER NOT NULL DEFAULT 0,
                worst_wan_rtt     REAL    NOT NULL DEFAULT 0,
                updated_at        INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS stability_buckets (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                day           TEXT    NOT NULL,          -- 'YYYY-MM-DD' (yerel saat)
                bucket_ts     INTEGER NOT NULL,          -- dakika başı epoch ms (UNIQUE anahtar)
                sample_count  INTEGER NOT NULL,          -- o dakikadaki örnek sayısı
                fail_count    INTEGER NOT NULL,          -- başarısız örnek (gw veya wan timeout)
                gw_avg        REAL,                      -- NULL = o dakika hiç gw yanıtı yok
                gw_min        REAL,
                gw_max        REAL,
                wan_avg       REAL,
                wan_min       REAL,
                wan_max       REAL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_bucket_ts ON stability_buckets(bucket_ts);
            CREATE INDEX IF NOT EXISTS idx_bucket_day ON stability_buckets(day);
            ALTER TABLE outage_events ADD COLUMN day TEXT;",
        )
        .map_err(|e| format!("v2 migration başarısız: {e}"))?;
    }

    if current < 3 {
        // v3: çok-modüllü test geçmişi. Tek esnek tablo — her modül metriklerini JSON yazar.
        // Yeni modül eklemek migration gerektirmez (tablo değişmez). Sadece SAYISAL GERÇEK
        // (metrics JSON ham metrik); skor saklanmaz — summary_score/grade istisna (trend için).
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS test_runs (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                ts             INTEGER NOT NULL,        -- test zamanı (epoch ms)
                day            TEXT    NOT NULL,        -- 'YYYY-MM-DD' (yerel saat)
                module         TEXT    NOT NULL,        -- 'full'|'speed'|'bufferbloat'|'ping'|...
                metrics        TEXT    NOT NULL,        -- JSON: modülün ham sayısal metrikleri
                summary_score  INTEGER,                 -- opsiyonel tek-sayı özet (trend), nullable
                summary_grade  TEXT,                    -- opsiyonel harf notu, nullable
                created_at     INTEGER NOT NULL         -- kaydın yazıldığı an (epoch ms)
            );
            CREATE INDEX IF NOT EXISTS idx_test_runs_module_ts ON test_runs(module, ts);
            CREATE INDEX IF NOT EXISTS idx_test_runs_ts ON test_runs(ts);"
        ).map_err(|e| format!("v3 migration başarısız: {e}"))?;
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|e| format!("user_version yazılamadı: {e}"))?;
    Ok(())
}

/// Bir kopma olayını kalıcı olarak kaydeder. Çağıran taraf bunu `spawn_blocking`
/// içinde çalıştırmalıdır (rusqlite bloklayıcıdır). Hata yutulmaz, döndürülür;
/// çağıran loglayıp devam edebilir (persist başarısızlığı monitörü durdurmamalı).
#[cfg(feature = "premium")]
pub fn record_outage(
    app: &AppHandle,
    ts: u64,
    kind: &str,
    phase: &str,
    duration_ms: Option<u64>,
) -> Result<(), String> {
    let path = db_path(app)?;
    let conn = open(&path)?;
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    insert_outage(&conn, ts, kind, phase, duration_ms, created_at)
}

/// Saf insert (AppHandle/yol bağımsız) — test edilebilir çekirdek.
#[cfg(feature = "premium")]
fn insert_outage(
    conn: &Connection,
    ts: u64,
    kind: &str,
    phase: &str,
    duration_ms: Option<u64>,
    created_at: i64,
) -> Result<(), String> {
    let day = day_label(ts);
    conn.execute(
        "INSERT INTO outage_events (ts, kind, phase, duration_ms, created_at, day)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![ts as i64, kind, phase, duration_ms.map(|d| d as i64), created_at, day],
    )
    .map_err(|e| format!("outage insert başarısız: {e}"))?;
    Ok(())
}

/// Bir dakikalık pencerede biriken ölçüm özeti. monitor_loop tarafınca doldurulur.
/// Yalnızca sayisal gerçek; skor/yorum yok.
#[cfg(feature = "premium")]
#[derive(Clone, Debug, Default)]
pub struct BucketData {
    pub bucket_ts: u64,    // dakika başı epoch ms
    pub sample_count: u32, // o dakikadaki toplam örnek
    pub fail_count: u32,   // gw VEYA wan timeout olan örnek sayısı
    pub gw_rtts: Vec<f64>, // başarılı gateway RTT'leri
    pub wan_rtts: Vec<f64>,// başarılı wan RTT'leri
}

#[cfg(feature = "premium")]
fn agg(v: &[f64]) -> (Option<f64>, Option<f64>, Option<f64>) {
    if v.is_empty() {
        return (None, None, None);
    }
    let sum: f64 = v.iter().sum();
    let avg = sum / v.len() as f64;
    let min = v.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    (Some(avg), Some(min), Some(max))
}

/// Bir dakikalık bucket'ı kalıcı yazar: stability_buckets'a satir + daily_stability upsert.
/// `spawn_blocking` içinde çağrılmalı. Best-effort.
#[cfg(feature = "premium")]
pub fn record_bucket(app: &AppHandle, b: &BucketData) -> Result<(), String> {
    let path = db_path(app)?;
    let conn = open(&path)?;
    insert_bucket(&conn, b)
}

/// Saf bucket yazimi (test edilebilir çekirdek): bucket insert + daily upsert.
#[cfg(feature = "premium")]
fn insert_bucket(conn: &Connection, b: &BucketData) -> Result<(), String> {
    if b.sample_count == 0 {
        return Ok(()); // boş bucket yazma
    }
    let day = day_label(b.bucket_ts);
    let (gw_avg, gw_min, gw_max) = agg(&b.gw_rtts);
    let (wan_avg, wan_min, wan_max) = agg(&b.wan_rtts);

    // INSERT OR REPLACE — bucket_ts UNIQUE; aynı dakika tekrar yazılırsa üzerine yazar.
    conn.execute(
        "INSERT OR REPLACE INTO stability_buckets
            (day, bucket_ts, sample_count, fail_count, gw_avg, gw_min, gw_max, wan_avg, wan_min, wan_max)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            day, b.bucket_ts as i64, b.sample_count, b.fail_count,
            gw_avg, gw_min, gw_max, wan_avg, wan_min, wan_max
        ],
    )
    .map_err(|e| format!("bucket insert başarısız: {e}"))?;

    // daily_stability upsert: o günün toplamlarını artırımlı güncelle.
    // monitored_ms: bu bucket'ın kapsadığı süre ≈ sample_count * INTERVAL (5sn). Burada
    // sabit varsaymak yerine örnek*5000 ms ekliyoruz (yaklaşık ama tutarlı).
    let gw_sum: f64 = b.gw_rtts.iter().sum();
    let wan_sum: f64 = b.wan_rtts.iter().sum();
    let worst_wan = wan_max.unwrap_or(0.0);
    let monitored_ms = b.sample_count as i64 * 5_000;
    let updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO daily_stability
            (day, monitored_ms, sample_count, fail_count, gw_rtt_sum, gw_rtt_n,
             wan_rtt_sum, wan_rtt_n, worst_wan_rtt, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(day) DO UPDATE SET
            monitored_ms  = monitored_ms + excluded.monitored_ms,
            sample_count  = sample_count + excluded.sample_count,
            fail_count    = fail_count   + excluded.fail_count,
            gw_rtt_sum    = gw_rtt_sum   + excluded.gw_rtt_sum,
            gw_rtt_n      = gw_rtt_n     + excluded.gw_rtt_n,
            wan_rtt_sum   = wan_rtt_sum  + excluded.wan_rtt_sum,
            wan_rtt_n     = wan_rtt_n    + excluded.wan_rtt_n,
            worst_wan_rtt = MAX(worst_wan_rtt, excluded.worst_wan_rtt),
            updated_at    = excluded.updated_at",
        rusqlite::params![
            day, monitored_ms, b.sample_count, b.fail_count,
            gw_sum, b.gw_rtts.len() as i64,
            wan_sum, b.wan_rtts.len() as i64,
            worst_wan, updated_at
        ],
    )
    .map_err(|e| format!("daily upsert başarısız: {e}"))?;
    Ok(())
}

/// Bir biten kopma olayının süresini ilgili günün outage sayılarına ekler.
/// `spawn_blocking` içinde çağrılmalı. Best-effort.
#[cfg(feature = "premium")]
pub fn record_outage_to_daily(app: &AppHandle, ts: u64, duration_ms: u64) -> Result<(), String> {
    let path = db_path(app)?;
    let conn = open(&path)?;
    let day = day_label(ts);
    conn.execute(
        "INSERT INTO daily_stability (day, outage_count, total_outage_ms, updated_at)
         VALUES (?1, 1, ?2, ?3)
         ON CONFLICT(day) DO UPDATE SET
            outage_count    = outage_count + 1,
            total_outage_ms = total_outage_ms + excluded.total_outage_ms,
            updated_at      = excluded.updated_at",
        rusqlite::params![
            day, duration_ms as i64,
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64).unwrap_or(0)
        ],
    )
    .map_err(|e| format!("outage->daily upsert başarısız: {e}"))?;
    Ok(())
}

/// Bir test çalıştırma kaydı (UI'ya JSON iletilir).
#[derive(Clone, Debug, serde::Serialize)]
pub struct TestRunRow {
    pub id: i64,
    pub ts: i64,
    pub day: String,
    pub module: String,
    pub metrics: String,        // JSON string (UI parse eder)
    pub summary_score: Option<i64>,
    pub summary_grade: Option<String>,
    pub created_at: i64,
}

/// Bir test çalıştırmasını kalıcı kaydeder. spawn_blocking içinde çağrılmalı. Best-effort.
pub fn record_test_run(
    app: &AppHandle,
    ts: u64,
    module: &str,
    metrics: &str,            // JSON string (frontend hazırlar)
    summary_score: Option<i64>,
    summary_grade: Option<&str>,
) -> Result<(), String> {
    let path = db_path(app)?;
    let conn = open(&path)?;
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    insert_test_run(&conn, ts, module, metrics, summary_score, summary_grade, created_at)
}

/// Saf insert (test edilebilir çekirdek).
fn insert_test_run(
    conn: &Connection,
    ts: u64,
    module: &str,
    metrics: &str,
    summary_score: Option<i64>,
    summary_grade: Option<&str>,
    created_at: i64,
) -> Result<(), String> {
    let day = day_label(ts);
    conn.execute(
        "INSERT INTO test_runs (ts, day, module, metrics, summary_score, summary_grade, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![ts as i64, day, module, metrics, summary_score, summary_grade, created_at],
    ).map_err(|e| format!("test_run insert başarısız: {e}"))?;
    Ok(())
}

/// module=None → tüm kayıtlar (ana geçmiş); Some(m) → sadece o modül. ts DESC, limit.
pub fn fetch_test_runs(
    app: &AppHandle,
    module: Option<&str>,
    limit: i64,
) -> Result<Vec<TestRunRow>, String> {
    let path = db_path(app)?;
    let conn = open(&path)?;
    select_test_runs(&conn, module, limit)
}

fn select_test_runs(
    conn: &Connection,
    module: Option<&str>,
    limit: i64,
) -> Result<Vec<TestRunRow>, String> {
    let lim = limit.clamp(1, 1000);
    let mut rows = Vec::new();
    if let Some(m) = module {
        let mut stmt = conn.prepare(
            "SELECT id,ts,day,module,metrics,summary_score,summary_grade,created_at
             FROM test_runs WHERE module=?1 ORDER BY ts DESC LIMIT ?2"
        ).map_err(|e| e.to_string())?;
        let it = stmt.query_map(rusqlite::params![m, lim], map_test_run_row).map_err(|e| e.to_string())?;
        for r in it { rows.push(r.map_err(|e| e.to_string())?); }
    } else {
        let mut stmt = conn.prepare(
            "SELECT id,ts,day,module,metrics,summary_score,summary_grade,created_at
             FROM test_runs ORDER BY ts DESC LIMIT ?1"
        ).map_err(|e| e.to_string())?;
        let it = stmt.query_map(rusqlite::params![lim], map_test_run_row).map_err(|e| e.to_string())?;
        for r in it { rows.push(r.map_err(|e| e.to_string())?); }
    }
    Ok(rows)
}

fn map_test_run_row(r: &rusqlite::Row) -> rusqlite::Result<TestRunRow> {
    Ok(TestRunRow {
        id: r.get(0)?, ts: r.get(1)?, day: r.get(2)?, module: r.get(3)?,
        metrics: r.get(4)?, summary_score: r.get(5)?, summary_grade: r.get(6)?, created_at: r.get(7)?,
    })
}

// ============= Tauri commands =============

#[tauri::command]
pub async fn save_test_run(
    app: tauri::AppHandle,
    ts: u64,
    module: String,
    metrics: String,
    summary_score: Option<i64>,
    summary_grade: Option<String>,
) -> Result<(), String> {
    let _ = tokio::task::spawn_blocking(move || {
        if let Err(e) = record_test_run(&app, ts, &module, &metrics, summary_score, summary_grade.as_deref()) {
            crate::dlog!("[db] test_run kaydedilemedi: {}", e);
        }
    }).await;
    Ok(()) // best-effort: hata testi durdurmaz
}

#[tauri::command]
pub async fn list_test_runs(
    app: tauri::AppHandle,
    limit: i64,
) -> Result<Vec<TestRunRow>, String> {
    tokio::task::spawn_blocking(move || fetch_test_runs(&app, None, limit))
        .await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_test_runs_by_module(
    app: tauri::AppHandle,
    module: String,
    limit: i64,
) -> Result<Vec<TestRunRow>, String> {
    tokio::task::spawn_blocking(move || fetch_test_runs(&app, Some(&module), limit))
        .await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_kurulur_ve_user_version_set_edilir() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        // tablo gerçekten var mı
        let cnt: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='outage_events'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt, 1);
    }

    #[test]
    fn migration_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // İkinci kez çalıştırmak hata vermemeli (CREATE IF NOT EXISTS)
        run_migrations(&conn).unwrap();
    }

    #[test]
    fn outage_insert_ve_okuma() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        insert_outage(&conn, 1000, "gateway", "start", None, 1234).unwrap();
        insert_outage(&conn, 2000, "gateway", "end", Some(1000), 2345).unwrap();

        let total: i64 = conn
            .query_row("SELECT count(*) FROM outage_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 2);

        // end olayında duration dolu, start'ta NULL
        let dur: Option<i64> = conn
            .query_row(
                "SELECT duration_ms FROM outage_events WHERE phase='end'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dur, Some(1000));

        let start_dur: Option<i64> = conn
            .query_row(
                "SELECT duration_ms FROM outage_events WHERE phase='start'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(start_dur, None);
    }

    #[test]
    fn day_label_yerel_saat_uretir() {
        // 0 ms = 1970-01-01 UTC; yerel saate göre 1969-12-31 veya 1970-01-01 olabilir.
        let d = day_label(0);
        assert!(d.starts_with("19"), "beklenen YYYY-MM-DD formatı, gelen: {d}");
        assert_eq!(d.len(), 10);
    }

    #[test]
    fn v2_tablolari_kurulur() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        for t in ["daily_stability", "stability_buckets"] {
            let cnt: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [t],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(cnt, 1, "{t} tablosu yok");
        }
        // outage_events'e day kolonu eklendi mi
        let has_day: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('outage_events') WHERE name='day'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_day, 1);
    }

    #[test]
    fn v1_den_v2_ye_yukseltme() {
        // Önce sadece v1 şemasını kur (eski sürüm simülasyonu)
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE outage_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, kind TEXT NOT NULL,
                phase TEXT NOT NULL, duration_ms INTEGER, created_at INTEGER NOT NULL);",
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 1i64).unwrap();
        // v1->v2->v3 migration: tüm adımlar sırayla çalışmalı
        run_migrations(&conn).unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 3);
        // Artık insert çalışmalı
        insert_outage(&conn, 1000, "wan", "end", Some(500), 1000).unwrap();
    }

    #[test]
    fn test_v3_test_runs_insert_and_select() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();  // v3 dahil tüm migration
        insert_test_run(&conn, 1000, "speed", "{\"dl\":100.0}", Some(85), Some("A"), 2000).unwrap();
        insert_test_run(&conn, 2000, "ping", "{\"avg\":12.0}", None, None, 3000).unwrap();
        let all = select_test_runs(&conn, None, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].module, "ping"); // ts DESC
        let speed = select_test_runs(&conn, Some("speed"), 10).unwrap();
        assert_eq!(speed.len(), 1);
        assert_eq!(speed[0].summary_grade.as_deref(), Some("A"));
    }

    #[test]
    fn test_migration_v2_to_v3_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap(); // tekrar çalışınca patlamamalı (IF NOT EXISTS)
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 3);
    }

    #[cfg(feature = "premium")]
    #[test]
    fn bucket_insert_ve_daily_upsert() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let b = BucketData {
            bucket_ts: 60_000, // dakika başı
            sample_count: 12,
            fail_count: 2,
            gw_rtts: vec![4.0, 6.0, 5.0],   // avg 5, min 4, max 6
            wan_rtts: vec![20.0, 40.0],     // avg 30, min 20, max 40
        };
        insert_bucket(&conn, &b).unwrap();

        // bucket satiri yazıldı
        let (gw_avg, wan_max): (f64, f64) = conn
            .query_row(
                "SELECT gw_avg, wan_max FROM stability_buckets WHERE bucket_ts=60000",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(gw_avg, 5.0);
        assert_eq!(wan_max, 40.0);

        // daily upsert: aynı güne ikinci bucket eklenince toplamlar artmalı
        let b2 = BucketData {
            bucket_ts: 120_000,
            sample_count: 12,
            fail_count: 0,
            gw_rtts: vec![10.0],
            wan_rtts: vec![50.0],
        };
        insert_bucket(&conn, &b2).unwrap();

        let (samples, fails, wan_n, worst): (i64, i64, i64, f64) = conn
            .query_row(
                "SELECT sample_count, fail_count, wan_rtt_n, worst_wan_rtt FROM daily_stability",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(samples, 24);  // 12+12
        assert_eq!(fails, 2);     // 2+0
        assert_eq!(wan_n, 3);     // 2+1 başarılı wan örneği
        assert_eq!(worst, 50.0);  // MAX(40,50)

        // tek gün satırı olmalı (gun-merkezli)
        let day_rows: i64 = conn
            .query_row("SELECT count(*) FROM daily_stability", [], |r| r.get(0))
            .unwrap();
        assert_eq!(day_rows, 1);
    }

    #[cfg(feature = "premium")]
    #[test]
    fn bos_bucket_yazilmaz() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        insert_bucket(&conn, &BucketData::default()).unwrap(); // sample_count=0
        let cnt: i64 = conn
            .query_row("SELECT count(*) FROM stability_buckets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, 0);
    }

    // Gerçek DOSYA DB'si: open() (WAL/pragma/migration) + insert zinciri diskte çalışıyor mu.
    #[cfg(feature = "premium")]
    #[test]
    fn gercek_dosya_db_uctan_uca() {
        let mut path = std::env::temp_dir();
        path.push(format!("netveil_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);

        // open(): WAL + migration uygular
        let conn = open(&path).unwrap();
        insert_outage(&conn, 1000, "wan", "start", None, 1000).unwrap();
        insert_bucket(
            &conn,
            &BucketData {
                bucket_ts: 60_000,
                sample_count: 12,
                fail_count: 0,
                gw_rtts: vec![3.0, 5.0],
                wan_rtts: vec![25.0],
            },
        )
        .unwrap();
        drop(conn);

        // Dosya gerçekten yazıldı ve yeniden açılabiliyor mu (kalıcılık)
        assert!(path.exists(), "db dosyası oluşmadı");
        let conn2 = open(&path).unwrap();
        let buckets: i64 = conn2
            .query_row("SELECT count(*) FROM stability_buckets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(buckets, 1);
        drop(conn2);

        // temizlik (WAL yan dosyaları dahil)
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
}
