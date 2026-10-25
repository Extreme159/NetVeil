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
//! İleriki adımlar (roadmap):
//! - Adım 2: `test_runs` tablosu + geçmiş karşılaştırma sorguları + Tauri komutları.
//! - Adım 3: `uploaded` flag tabanlı sunucu senkron kuyruğu.

use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Mevcut şema versiyonu. Yeni migration eklerken artır ve `run_migrations`'a ekle.
const SCHEMA_VERSION: i64 = 4;

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
    // Fast path: already at (or beyond) this schema version -> no write lock, nothing touched.
    let existing: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| format!("user_version read failed: {e}"))?;
    if existing >= SCHEMA_VERSION {
        return Ok(());
    }
    // Version check + every schema change + the user_version bump run in ONE write transaction
    // (IMMEDIATE: concurrent opens serialize on the lock). An interruption rolls everything back,
    // so the DB can never be left half-migrated (e.g. column added but version not bumped).
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| format!("migration transaction could not be started: {e}"))?;
    let conn: &Connection = &tx;
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
        // Idempotent ALTER: a database broken by the old non-transactional migration already has
        // the column but a stale user_version; re-adding it would fail forever.
        if !column_exists(conn, "outage_events", "day")? {
            conn.execute_batch("ALTER TABLE outage_events ADD COLUMN day TEXT;")
                .map_err(|e| format!("v2 migration (day column) failed: {e}"))?;
        }
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
            CREATE INDEX IF NOT EXISTS idx_bucket_day ON stability_buckets(day);",
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

    if current < 4 {
        // v4: per-bucket gateway/WAN sample counts. A minute bucket can be written in two
        // fragments (monitor restarted within the same minute); the counts let the upsert merge
        // the averages with correct weights. Idempotent (column_exists) like every ALTER here.
        for col in ["gw_n", "wan_n"] {
            if !column_exists(conn, "stability_buckets", col)? {
                conn.execute_batch(&format!(
                    "ALTER TABLE stability_buckets ADD COLUMN {col} INTEGER NOT NULL DEFAULT 0;"
                ))
                .map_err(|e| format!("v4 migration ({col}) failed: {e}"))?;
                // Backfill ONLY the column this run just added (a pre-existing column from an
                // interrupted migration may already hold real counts). Pre-v4 buckets must not
                // get zero weight in the merge. fail_count counts samples where gw OR wan failed,
                // so sample_count - fail_count is a lower bound (samples where both replied) of
                // the segment's real count; at least 1 when the bucket has an average.
                let avg = if col == "gw_n" { "gw_avg" } else { "wan_avg" };
                conn.execute_batch(&format!(
                    "UPDATE stability_buckets
                     SET {col} = MAX(sample_count - fail_count, 1)
                     WHERE {avg} IS NOT NULL;"
                ))
                .map_err(|e| format!("v4 migration ({col} backfill) failed: {e}"))?;
            }
        }
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|e| format!("user_version yazılamadı: {e}"))?;
    tx.commit()
        .map_err(|e| format!("migration commit failed: {e}"))?;
    Ok(())
}

/// True when `table` already has a column named `column` (pragma_table_info).
fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info(?1) WHERE name = ?2",
            rusqlite::params![table, column],
            |r| r.get(0),
        )
        .map_err(|e| format!("table_info read failed: {e}"))?;
    Ok(n > 0)
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

    // Bucket row + daily totals are ONE transaction: either both are written or neither.
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("bucket transaction could not be started: {e}"))?;

    // UPSERT (bucket_ts UNIQUE): if the same minute is written again (monitor restarted within
    // the minute) the fragments are MERGED - counts add, min/max combine, averages are weighted
    // by the per-bucket sample counts - instead of the later fragment replacing the earlier one.
    tx.execute(
        "INSERT INTO stability_buckets
            (day, bucket_ts, sample_count, fail_count, gw_avg, gw_min, gw_max, wan_avg, wan_min, wan_max, gw_n, wan_n)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(bucket_ts) DO UPDATE SET
            sample_count = sample_count + excluded.sample_count,
            fail_count   = fail_count   + excluded.fail_count,
            gw_avg  = CASE WHEN excluded.gw_n = 0 THEN gw_avg
                           ELSE (COALESCE(gw_avg, 0) * (CASE WHEN gw_n = 0 AND gw_avg IS NOT NULL THEN 1 ELSE gw_n END)
                                 + excluded.gw_avg * excluded.gw_n)
                                / ((CASE WHEN gw_n = 0 AND gw_avg IS NOT NULL THEN 1 ELSE gw_n END) + excluded.gw_n) END,
            gw_min  = CASE WHEN gw_min IS NULL THEN excluded.gw_min
                           WHEN excluded.gw_min IS NULL THEN gw_min
                           ELSE MIN(gw_min, excluded.gw_min) END,
            gw_max  = CASE WHEN gw_max IS NULL THEN excluded.gw_max
                           WHEN excluded.gw_max IS NULL THEN gw_max
                           ELSE MAX(gw_max, excluded.gw_max) END,
            wan_avg = CASE WHEN excluded.wan_n = 0 THEN wan_avg
                           ELSE (COALESCE(wan_avg, 0) * (CASE WHEN wan_n = 0 AND wan_avg IS NOT NULL THEN 1 ELSE wan_n END)
                                 + excluded.wan_avg * excluded.wan_n)
                                / ((CASE WHEN wan_n = 0 AND wan_avg IS NOT NULL THEN 1 ELSE wan_n END) + excluded.wan_n) END,
            wan_min = CASE WHEN wan_min IS NULL THEN excluded.wan_min
                           WHEN excluded.wan_min IS NULL THEN wan_min
                           ELSE MIN(wan_min, excluded.wan_min) END,
            wan_max = CASE WHEN wan_max IS NULL THEN excluded.wan_max
                           WHEN excluded.wan_max IS NULL THEN wan_max
                           ELSE MAX(wan_max, excluded.wan_max) END,
            gw_n    = (CASE WHEN gw_n = 0 AND gw_avg IS NOT NULL THEN 1 ELSE gw_n END) + excluded.gw_n,
            wan_n   = (CASE WHEN wan_n = 0 AND wan_avg IS NOT NULL THEN 1 ELSE wan_n END) + excluded.wan_n",
        rusqlite::params![
            day, b.bucket_ts as i64, b.sample_count, b.fail_count,
            gw_avg, gw_min, gw_max, wan_avg, wan_min, wan_max,
            b.gw_rtts.len() as i64, b.wan_rtts.len() as i64
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

    tx.execute(
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
    tx.commit()
        .map_err(|e| format!("bucket commit failed: {e}"))?;
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
        if let Err(_e) = record_test_run(&app, ts, &module, &metrics, summary_score, summary_grade.as_deref()) {
            crate::dlog!("[db] test_run kaydedilemedi: {}", _e);
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
    #[cfg(feature = "premium")]
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
    #[cfg(feature = "premium")]
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
        assert_eq!(v, SCHEMA_VERSION);
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
        assert_eq!(v, SCHEMA_VERSION);
    }

    fn user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
    }

    /// Interrupted old migration: `day` column already added, user_version still 1.
    /// Re-opening must recover (idempotent ALTER) instead of failing with "duplicate column".
    #[test]
    fn migration_recovers_column_added_but_version_not_bumped() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE outage_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, kind TEXT NOT NULL,
                phase TEXT NOT NULL, duration_ms INTEGER, created_at INTEGER NOT NULL,
                day TEXT);",
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 1i64).unwrap();

        run_migrations(&conn).expect("must recover from a half-applied v2 migration");
        assert_eq!(user_version(&conn), SCHEMA_VERSION);
        assert!(column_exists(&conn, "outage_events", "day").unwrap());
        // the full schema is there
        for t in ["daily_stability", "stability_buckets", "test_runs"] {
            let n: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "{t} missing");
        }
        // and it stays openable
        run_migrations(&conn).unwrap();
    }

    /// Same recovery through the real file-backed open() path (reopen after the "crash").
    #[test]
    fn open_recovers_half_migrated_file_db() {
        let mut path = std::env::temp_dir();
        path.push(format!("netveil_halfmig_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE outage_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, kind TEXT NOT NULL,
                    phase TEXT NOT NULL, duration_ms INTEGER, created_at INTEGER NOT NULL, day TEXT);",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1i64).unwrap();
        }
        let conn = open(&path).expect("reopen must succeed");
        assert_eq!(user_version(&conn), SCHEMA_VERSION);
        drop(conn);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    /// A failing migration step must roll back everything (no half-applied schema, version intact).
    #[test]
    fn migration_failure_rolls_back_everything() {
        let conn = Connection::open_in_memory().unwrap();
        // An index named like a table the v2 step creates makes that step fail midway
        // (CREATE TABLE IF NOT EXISTS does not tolerate an index with the same name).
        conn.execute_batch(
            "CREATE TABLE decoy (x INTEGER);
             CREATE INDEX stability_buckets ON decoy(x);",
        )
        .unwrap();
        assert!(run_migrations(&conn).is_err());
        assert_eq!(user_version(&conn), 0, "version must not advance on failure");
        // Tables from earlier steps of the same run were rolled back too.
        for t in ["outage_events", "daily_stability"] {
            let n: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{t} must be rolled back");
        }
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

    /// Monitor restarted inside the same minute: the two fragments must MERGE, not overwrite.
    #[cfg(feature = "premium")]
    #[test]
    fn same_minute_fragments_merge() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let first = BucketData {
            bucket_ts: 60_000,
            sample_count: 4,
            fail_count: 1,
            gw_rtts: vec![2.0, 4.0],       // n=2 avg 3 min 2 max 4
            wan_rtts: vec![20.0],          // n=1 avg 20
        };
        let second = BucketData {
            bucket_ts: 60_000,
            sample_count: 3,
            fail_count: 2,
            gw_rtts: vec![9.0, 9.0],       // n=2 avg 9 min 9 max 9
            wan_rtts: vec![],              // no WAN reply in this fragment
        };
        insert_bucket(&conn, &first).unwrap();
        insert_bucket(&conn, &second).unwrap();

        let (n, fails, gw_avg, gw_min, gw_max, wan_avg, wan_max, gw_n, wan_n): (
            i64, i64, f64, f64, f64, f64, f64, i64, i64,
        ) = conn
            .query_row(
                "SELECT sample_count, fail_count, gw_avg, gw_min, gw_max, wan_avg, wan_max, gw_n, wan_n
                 FROM stability_buckets WHERE bucket_ts=60000",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
            )
            .unwrap();
        assert_eq!(n, 7);
        assert_eq!(fails, 3);
        assert_eq!(gw_n, 4);
        assert_eq!(wan_n, 1);
        assert!((gw_avg - 6.0).abs() < 1e-9, "weighted gw avg (3*2+9*2)/4 = 6, got {gw_avg}");
        assert_eq!(gw_min, 2.0);
        assert_eq!(gw_max, 9.0);
        assert_eq!(wan_avg, 20.0, "WAN avg must survive a fragment without WAN samples");
        assert_eq!(wan_max, 20.0);

        // exactly one bucket row, daily totals are the sum of both fragments
        let rows: i64 = conn.query_row("SELECT count(*) FROM stability_buckets", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1);
        let (ds, df): (i64, i64) = conn
            .query_row("SELECT sample_count, fail_count FROM daily_stability", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((ds, df), (7, 3));
    }

    /// v3 -> v4 migration of a populated bucket: gw_n/wan_n are backfilled (not 0), so the legacy
    /// averages keep their weight when a same-minute fragment is merged afterwards.
    #[cfg(feature = "premium")]
    #[test]
    fn v4_migration_backfills_counts_and_merge_weights_legacy_bucket() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // Rewind to a v3 database: no gw_n / wan_n columns.
        conn.execute_batch(
            "ALTER TABLE stability_buckets DROP COLUMN gw_n;
             ALTER TABLE stability_buckets DROP COLUMN wan_n;
             INSERT INTO stability_buckets
                (day, bucket_ts, sample_count, fail_count, gw_avg, gw_min, gw_max, wan_avg, wan_min, wan_max)
             VALUES ('1970-01-01', 60000, 10, 2, 10.0, 8.0, 12.0, 20.0, 15.0, 25.0),
                    ('1970-01-01', 120000, 6, 6, NULL, NULL, NULL, NULL, NULL, NULL);",
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 3i64).unwrap();

        run_migrations(&conn).unwrap();
        assert_eq!(user_version(&conn), SCHEMA_VERSION);

        let counts = |ts: i64| -> (i64, i64) {
            conn.query_row(
                "SELECT gw_n, wan_n FROM stability_buckets WHERE bucket_ts=?1",
                [ts],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(counts(60_000), (8, 8), "sample_count - fail_count");
        assert_eq!(counts(120_000), (0, 0), "no average -> no weight");

        // Merge a fragment into the legacy bucket: legacy avg weighs 8, fragment 2.
        insert_bucket(
            &conn,
            &BucketData {
                bucket_ts: 60_000,
                sample_count: 2,
                fail_count: 0,
                gw_rtts: vec![20.0, 20.0],
                wan_rtts: vec![],
            },
        )
        .unwrap();
        let (gw_avg, wan_avg): (f64, f64) = conn
            .query_row(
                "SELECT gw_avg, wan_avg FROM stability_buckets WHERE bucket_ts=60000",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((gw_avg - 12.0).abs() < 1e-9, "(10*8 + 20*2)/10 = 12, got {gw_avg}");
        assert_eq!(wan_avg, 20.0);
        assert_eq!(counts(60_000), (10, 8));
    }

    /// A row with n=0 but a non-null average (legacy data) counts as weight 1 in the merge, not 0.
    #[cfg(feature = "premium")]
    #[test]
    fn merge_treats_zero_count_with_average_as_weight_one() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO stability_buckets
                (day, bucket_ts, sample_count, fail_count, gw_avg, gw_min, gw_max, wan_avg, wan_min, wan_max, gw_n, wan_n)
             VALUES ('1970-01-01', 60000, 5, 0, 10.0, 10.0, 10.0, NULL, NULL, NULL, 0, 0);",
        )
        .unwrap();
        insert_bucket(
            &conn,
            &BucketData {
                bucket_ts: 60_000,
                sample_count: 1,
                fail_count: 0,
                gw_rtts: vec![30.0],
                wan_rtts: vec![40.0],
            },
        )
        .unwrap();
        let (gw_avg, wan_avg, gw_n, wan_n): (f64, f64, i64, i64) = conn
            .query_row(
                "SELECT gw_avg, wan_avg, gw_n, wan_n FROM stability_buckets WHERE bucket_ts=60000",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert!((gw_avg - 20.0).abs() < 1e-9, "(10*1 + 30*1)/2 = 20, got {gw_avg}");
        assert_eq!(gw_n, 2);
        assert_eq!(wan_avg, 40.0, "NULL legacy WAN avg: fragment value taken as is");
        assert_eq!(wan_n, 1);
    }

    /// If the daily upsert fails, the bucket row must not be left behind (one transaction).
    #[cfg(feature = "premium")]
    #[test]
    fn bucket_and_daily_are_atomic() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute_batch("DROP TABLE daily_stability;").unwrap();

        let b = BucketData {
            bucket_ts: 60_000,
            sample_count: 5,
            fail_count: 0,
            gw_rtts: vec![1.0],
            wan_rtts: vec![2.0],
        };
        assert!(insert_bucket(&conn, &b).is_err());
        let rows: i64 = conn.query_row("SELECT count(*) FROM stability_buckets", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 0, "bucket insert must be rolled back with the failed daily upsert");
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
