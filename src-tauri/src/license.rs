/// NetVeil License Management — LemonSqueezy API + keyring cache + offline grace
///
/// Güvenlik hardcode değerleri:
///   store_id   = 398034
///   product_id = 1125611
///   variant_id = 1761731 (monthly/30day) | 1761851 (lifetime)
///
/// Saklama: keyring crate ("NetVeil" / "license") — Windows Credential Store
///   Blocking I/O → tokio::task::spawn_blocking ile sarılır (async runtime bloke edilmez)
///
/// Offline grace: 7 gün — son başarılı doğrulamadan bu kadar süre sonra cache geçerli sayılır
use keyring::Entry;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

// ─── Güvenlik sabitleri ───────────────────────────────────────────────────────

const LS_STORE_ID: u64 = 398034;
const LS_PRODUCT_ID: u64 = 1125611;
const LS_VARIANT_MONTHLY: u64 = 1761731;
const LS_VARIANT_LIFETIME: u64 = 1761851;

const KEYRING_SERVICE: &str = "NetVeil";
const KEYRING_USER: &str = "license";

const GRACE_SECS: i64 = 7 * 24 * 3600; // 7 gün

const LS_ACTIVATE_URL: &str = "https://api.lemonsqueezy.com/v1/licenses/activate";
const LS_VALIDATE_URL: &str = "https://api.lemonsqueezy.com/v1/licenses/validate";
const LS_DEACTIVATE_URL: &str = "https://api.lemonsqueezy.com/v1/licenses/deactivate";

// ─── IPC tipi (UI'a döner) ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LicenseStatus {
    pub valid: bool,
    pub license_key: Option<String>,
    /// "none" | "monthly" | "lifetime"
    pub license_type: String,
    /// "active" | "inactive" | "expired" | "disabled" | "none" | "grace"
    pub status: String,
    pub activated_at: Option<String>,
    pub expires_at: Option<String>,
    pub error: Option<String>,
}

// ─── Keyring cache tipi ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LicenseCache {
    pub license_key: String,
    pub instance_id: String,
    pub valid: bool,
    pub license_type: String,   // "monthly" | "lifetime"
    pub status: String,
    pub expires_at: Option<String>,
    pub last_validated_at: i64, // Unix timestamp (saniye)
}

// ─── LemonSqueezy API yanıt tipleri ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LsLicenseKey {
    pub status: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LsInstance {
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LsMeta {
    pub store_id: Option<u64>,
    pub product_id: Option<u64>,
    pub variant_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ActivateResponse {
    pub activated: Option<bool>,
    pub error: Option<String>,
    pub license_key: Option<LsLicenseKey>,
    pub instance: Option<LsInstance>,
    pub meta: Option<LsMeta>,
}

#[derive(Debug, Deserialize)]
struct ValidateResponse {
    pub valid: Option<bool>,
    pub error: Option<String>,
    pub license_key: Option<LsLicenseKey>,
}

#[derive(Debug, Deserialize)]
struct DeactivateResponse {
    pub deactivated: Option<bool>,
    pub error: Option<String>,
}

// ─── Yardımcı fonksiyonlar ────────────────────────────────────────────────────

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn build_client() -> Result<Client, String> {
    Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}

/// Hostname al — dns_lookup::get_hostname, hata olursa "NetVeil-Device" döner
fn instance_name() -> String {
    dns_lookup::get_hostname().unwrap_or_else(|_| "NetVeil-Device".to_string())
}

/// variant_id'den lisans tipini belirle
fn variant_to_type(variant_id: u64) -> Option<String> {
    match variant_id {
        LS_VARIANT_MONTHLY => Some("monthly".to_string()),
        LS_VARIANT_LIFETIME => Some("lifetime".to_string()),
        _ => None,
    }
}

/// keyring'den LicenseCache oku (spawn_blocking ile)
async fn read_cache() -> Option<LicenseCache> {
    tokio::task::spawn_blocking(|| {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
        let json = entry.get_password().ok()?;
        serde_json::from_str::<LicenseCache>(&json).ok()
    })
    .await
    .unwrap_or(None)
}

/// keyring'e LicenseCache yaz (spawn_blocking ile)
async fn write_cache(cache: &LicenseCache) -> Result<(), String> {
    let json = serde_json::to_string(cache).map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| e.to_string())?;
        entry.set_password(&json).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// keyring'deki kaydı sil (spawn_blocking ile)
async fn delete_cache() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| e.to_string())?;
        entry.delete_credential().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// LicenseCache'i LicenseStatus'a dönüştür (online doğrulama sonrası veya grace için)
fn cache_to_status(cache: &LicenseCache, grace: bool) -> LicenseStatus {
    LicenseStatus {
        valid: cache.valid,
        license_key: Some(mask_key(&cache.license_key)),
        license_type: cache.license_type.clone(),
        status: if grace { "grace".to_string() } else { cache.status.clone() },
        activated_at: None,
        expires_at: cache.expires_at.clone(),
        error: None,
    }
}

/// Lisans anahtarının ortasını maskele (güvenlik: UI'da tam key gösterme)
fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return key.to_string();
    }
    let (prefix, rest) = key.split_at(4);
    let suffix = &rest[rest.len().saturating_sub(4)..];
    format!("{}-****-****-****-{}", prefix, suffix)
}

/// 7 günlük grace süresi doldu mu kontrol et
fn is_within_grace(last_validated_at: i64) -> bool {
    now_unix() - last_validated_at < GRACE_SECS
}

// ─── Premium guard ───────────────────────────────────────────────────────────

#[cfg(feature = "premium")]
pub async fn ensure_licensed() -> Result<(), String> {
    let cache = match read_cache().await {
        Some(c) => c,
        None => return Err("License required.".to_string()),
    };
    if cache.valid && is_within_grace(cache.last_validated_at) {
        return Ok(());
    }
    Err("License required.".to_string())
}

// ─── Tauri komutları ──────────────────────────────────────────────────────────

/// Hızlı durum sorgulama — NETWORK YOK. keyring'den okur, grace kararı verir.
/// UI açılışında veya sekme yüklendiğinde çağrılır.
#[tauri::command]
pub async fn get_license_status(_app: AppHandle) -> LicenseStatus {
    let cache = match read_cache().await {
        Some(c) => c,
        None => {
            return LicenseStatus {
                valid: false,
                license_key: None,
                license_type: "none".to_string(),
                status: "none".to_string(),
                activated_at: None,
                expires_at: None,
                error: None,
            };
        }
    };

    // Cache var: geçerli mi VE grace süresi içinde mi?
    if cache.valid && is_within_grace(cache.last_validated_at) {
        return cache_to_status(&cache, false);
    }

    // Cache geçersiz veya grace süresi dolmuş
    LicenseStatus {
        valid: false,
        license_key: Some(mask_key(&cache.license_key)),
        license_type: cache.license_type.clone(),
        status: if !cache.valid { cache.status.clone() } else { "expired".to_string() },
        activated_at: None,
        expires_at: cache.expires_at.clone(),
        error: Some("License validation required.".to_string()),
    }
}

/// Lisans aktivasyonu — LemonSqueezy /activate → meta doğrula → keyring'e kaydet.
#[tauri::command]
pub async fn activate_license(_app: AppHandle, key: String) -> Result<LicenseStatus, String> {
    crate::dlog!("[license] activate çağrıldı");

    let client = build_client()?;
    let hostname = instance_name();

    let params = [
        ("license_key", key.as_str()),
        ("instance_name", hostname.as_str()),
    ];

    let resp = client
        .post(LS_ACTIVATE_URL)
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let body: ActivateResponse = resp
        .json()
        .await
        .map_err(|e| format!("Response parse error: {}", e))?;

    // API hata kontrolü
    if body.activated != Some(true) {
        let err = body.error.unwrap_or_else(|| "Activation failed.".to_string());
        return Err(err);
    }

    // Meta güvenlik doğrulaması
    let meta = body.meta.ok_or("Server meta data missing.")?;

    if meta.store_id != Some(LS_STORE_ID) || meta.product_id != Some(LS_PRODUCT_ID) {
        return Err("Invalid product: this license is not for this app.".to_string());
    }

    let variant_id = meta.variant_id.ok_or("Variant ID missing.")?;
    let license_type = variant_to_type(variant_id)
        .ok_or("Invalid product variant.".to_string())?;

    // Instance ID al
    let instance = body.instance.ok_or("Instance info missing.")?;
    let instance_id = instance.id.ok_or("Instance ID missing.")?;

    // Lisans key bilgisi
    let lk = body.license_key.unwrap_or(LsLicenseKey { status: None, expires_at: None });
    let status = lk.status.unwrap_or_else(|| "active".to_string());
    let expires_at = lk.expires_at;

    // Cache kaydet
    let cache = LicenseCache {
        license_key: key.clone(),
        instance_id,
        valid: true,
        license_type: license_type.clone(),
        status: status.clone(),
        expires_at: expires_at.clone(),
        last_validated_at: now_unix(),
    };
    write_cache(&cache).await?;

    crate::dlog!("[license] aktivasyon başarılı: {} ({})", license_type, status);

    Ok(LicenseStatus {
        valid: true,
        license_key: Some(mask_key(&key)),
        license_type,
        status,
        activated_at: None,
        expires_at,
        error: None,
    })
}

/// Online doğrulama — keyring'den key+instance_id oku → LemonSqueezy /validate.
/// Ağ hatasında cache'e DOKUNMAZ (grace devam eder). Başarılıda last_validated_at günceller.
#[tauri::command]
pub async fn validate_license(_app: AppHandle) -> LicenseStatus {
    let cache = match read_cache().await {
        Some(c) => c,
        None => {
            return LicenseStatus {
                valid: false,
                license_key: None,
                license_type: "none".to_string(),
                status: "none".to_string(),
                activated_at: None,
                expires_at: None,
                error: Some("No saved license found.".to_string()),
            };
        }
    };

    let client = match build_client() {
        Ok(c) => c,
        Err(_e) => {
            // Client oluşturulamadı — grace devam eder
            crate::dlog!("[license] reqwest client hatası: {}", _e);
            let grace = cache.valid && is_within_grace(cache.last_validated_at);
            return cache_to_status(&cache, grace);
        }
    };

    let params = [
        ("license_key", cache.license_key.clone()),
        ("instance_id", cache.instance_id.clone()),
    ];

    let resp = client
        .post(LS_VALIDATE_URL)
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(_e) => {
            // Ağ hatası → cache'e dokunma, grace state döndür
            crate::dlog!("[license] ağ hatası (grace devam): {}", _e);
            let grace = cache.valid && is_within_grace(cache.last_validated_at);
            return cache_to_status(&cache, grace);
        }
    };

    let body: ValidateResponse = match resp.json().await {
        Ok(b) => b,
        Err(_e) => {
            crate::dlog!("[license] parse hatası (grace devam): {}", _e);
            let grace = cache.valid && is_within_grace(cache.last_validated_at);
            return cache_to_status(&cache, grace);
        }
    };

    let api_valid = body.valid.unwrap_or(false);
    let lk = body.license_key.unwrap_or(LsLicenseKey { status: None, expires_at: None });
    let api_status = lk.status.unwrap_or_else(|| "inactive".to_string());
    let api_expires = lk.expires_at.or_else(|| cache.expires_at.clone());

    if api_valid {
        // Başarılı → cache güncelle
        let updated = LicenseCache {
            license_key: cache.license_key.clone(),
            instance_id: cache.instance_id.clone(),
            valid: true,
            license_type: cache.license_type.clone(),
            status: api_status.clone(),
            expires_at: api_expires.clone(),
            last_validated_at: now_unix(),
        };
        if let Err(_e) = write_cache(&updated).await {
            crate::dlog!("[license] cache yazma hatası: {}", _e);
        }
        LicenseStatus {
            valid: true,
            license_key: Some(mask_key(&cache.license_key)),
            license_type: cache.license_type.clone(),
            status: api_status,
            activated_at: None,
            expires_at: api_expires,
            error: None,
        }
    } else {
        // valid=false (expired/disabled/revoked) → cache'i geçersiz işaretle
        let err_msg = body.error.unwrap_or_else(|| "License invalid.".to_string());
        let updated = LicenseCache {
            license_key: cache.license_key.clone(),
            instance_id: cache.instance_id.clone(),
            valid: false,
            license_type: cache.license_type.clone(),
            status: api_status.clone(),
            expires_at: api_expires.clone(),
            last_validated_at: cache.last_validated_at, // eski zaman damgası korunur
        };
        if let Err(_e) = write_cache(&updated).await {
            crate::dlog!("[license] cache yazma hatası: {}", _e);
        }
        LicenseStatus {
            valid: false,
            license_key: Some(mask_key(&cache.license_key)),
            license_type: cache.license_type.clone(),
            status: api_status,
            activated_at: None,
            expires_at: api_expires,
            error: Some(err_msg),
        }
    }
}

/// Lisans deaktivasyonu — LemonSqueezy /deactivate → keyring'i temizle.
#[tauri::command]
pub async fn deactivate_license(_app: AppHandle) -> Result<(), String> {
    crate::dlog!("[license] deactivate çağrıldı");

    let cache = read_cache()
        .await
        .ok_or("No saved license found.")?;

    let client = build_client()?;
    let params = [
        ("license_key", cache.license_key.clone()),
        ("instance_id", cache.instance_id.clone()),
    ];

    let resp = client
        .post(LS_DEACTIVATE_URL)
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let body: DeactivateResponse = resp
        .json()
        .await
        .map_err(|e| format!("Response parse error: {}", e))?;

    if body.deactivated != Some(true) {
        let err = body.error.unwrap_or_else(|| "Deactivation failed.".to_string());
        return Err(err);
    }

    delete_cache().await?;
    crate::dlog!("[license] deaktivasyon başarılı, cache temizlendi");

    Ok(())
}
