use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DiagnosticInput {
    pub nic: serde_json::Value,
    pub wifi: serde_json::Value,
    pub ping_results: serde_json::Value,
    pub bufferbloat: serde_json::Value,
    pub speed_download: Option<f64>,
    pub speed_upload: Option<f64>,
    pub throttle: Option<serde_json::Value>,
    pub isp_intervention: Option<serde_json::Value>,
    pub ipv6: Option<serde_json::Value>,
    #[serde(default)]
    pub cgnat: Option<serde_json::Value>,
    #[serde(default)]
    pub mtu: Option<serde_json::Value>,
    #[serde(default)]
    pub wifi_scan: Option<serde_json::Value>,
    #[serde(default)]
    pub voip: Option<serde_json::Value>,
    #[serde(default)]
    pub dns_benchmark: Option<serde_json::Value>,
    #[serde(default)]
    pub twitch: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SmartDiagnosis {
    pub summary: String,
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
    /// Sadece tam test (speed+throttle+isp+ipv6) yapıldıysa dolu gelir
    pub general_score: Option<GeneralScore>,
    pub source: String, // her zaman "local" — lokal Smart Diagnosis motoru
    /// Modul-modul yapilandirilmis yorumlar (Smart Diagnosis Engine, Faz 3).
    pub modules: Vec<ModuleVerdict>,
    /// Toptan degerlendirme durumu: "excellent"|"good"|"fair"|"poor"|"critical"
    pub overall_status: String,
    /// Buyuk resim sentezi — genel ag kalitesi (yalnizca tam testte).
    pub general_synthesis: Option<Synthesis>,
}

/// FPS oyuncu skoru — sıkı gecikme/jitter eşikleri, 5 bileşen.
#[derive(Debug, Serialize, Deserialize)]
pub struct FpsScore {
    pub latency: u32,
    pub jitter: u32,
    pub packet_loss: u32,
    pub bufferbloat: u32,
    pub wifi: u32,
    pub total: u32,
    pub grade: String,
}

/// FPS tanı sonucu — FpsScore + Synthesis + ilgili ModuleVerdict'ler.
#[derive(Debug, Serialize, Deserialize)]
pub struct FpsDiagnosis {
    pub fps_score: FpsScore,
    pub fps_synthesis: Synthesis,
    pub modules: Vec<ModuleVerdict>,
}

/// Buyuk resim sentezi: skor + harf notu + headline + pozitif/notr/negatif icgoruler.
/// Modullerin kombinasyonundan turetilir (deterministik).
#[derive(Debug, Serialize, Deserialize)]
pub struct Synthesis {
    pub score: u32,
    pub grade: String,
    pub headline_key: String,   // i18n: tek cumle buyuk resim ozeti
    pub insights: Vec<Insight>, // pozitif/notr/negatif sentez maddeleri
}

/// Tek bir test modulunun yapilandirilmis yorumu.
/// Backend metin URETMEZ — sadece id+skor+grade+metrik+i18n anahtari doner;
/// frontend t(detail_key, params) ile cevirir (EN-oncelikli, cok-dilli).
#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleVerdict {
    pub module: String,       // stable id: "latency" | "jitter" | ...
    pub score: u32,           // 0-100
    pub grade: String,        // A+/A/B/C/D/F
    pub metric: f64,          // olculen ham deger
    pub metric_unit: String,  // "ms" | "%" | "Mbps" | ""
    pub detail_key: String,   // i18n anahtari, orn. "diag_latency_A"
    pub available: bool,      // bu modul bu testte olculdu mu
    /// Zengin engine yorumu: pozitif/notr/negatif icgoruler (Faz 3b).
    /// Bos olabilir (henuz icgoru tanimlanmamis moduller).
    pub insights: Vec<Insight>,
}

/// Tek bir yapilandirilmis icgoru. Backend metin uretmez; frontend t(text_key) cevirir.
#[derive(Debug, Serialize, Deserialize)]
pub struct Insight {
    pub kind: String,      // "positive" | "neutral" | "negative"
    pub text_key: String,  // i18n anahtari, orn. "insight_latency_competitive"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GamingScore {
    pub latency: u32,
    pub jitter: u32,
    pub bufferbloat: u32,
    pub packet_loss: u32,
    pub total: u32,
    pub grade: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneralScore {
    pub latency: u32,
    pub jitter: u32,
    pub bufferbloat: u32,
    pub packet_loss: u32,
    pub speed: Option<u32>,
    pub wifi: u32,
    pub throttle: Option<u32>,
    pub isp_intervention: Option<u32>,
    pub ipv6: Option<u32>,
    pub cgnat: Option<u32>,
    pub mtu: Option<u32>,
    pub voip: Option<u32>,
    pub dns_benchmark: Option<u32>,
    pub twitch: Option<u32>,
    pub total: u32,
    pub grade: String,
}

#[tauri::command]
pub async fn run_smart_diagnosis(input: DiagnosticInput) -> Result<SmartDiagnosis, String> {
    // Genel skoru her zaman hesapla.
    // compute_general_score opsiyonel bileşenleri (if let Some) null-güvenli işler;
    // pro alanlar null ise yalnızca 5 çekirdek bileşenle (ağırlık 50, normalize) skor üretir.
    let general_score = Some(compute_general_score(&input));

    // Lokal Smart Diagnosis motoru — deterministik, offline (LLM yok).
    // local_analyze artık sadece genel skor için kullanılır (gaming_score kaldırıldı)
    let gaming_fallback = compute_gaming_score(&input);
    let (summary, issues, suggestions, source) = local_analyze(&input, &gaming_fallback);

    // Modul-modul yapilandirilmis yorumlar (Faz 3 Smart Diagnosis Engine).
    // Twitch One-Click'te OLCULMEZ (nis yayinci ozelligi) -> modul listesinden cikar.
    // NOT: run_module_verdict twitch'i hala uretir (Twitch kendi sayfasinda grade gosterir).
    let modules: Vec<ModuleVerdict> = build_module_verdicts(&input)
        .into_iter()
        .filter(|m| m.module != "twitch")
        .collect();
    let overall_status = compute_overall_status(&modules);

    // Buyuk resim sentezi — sadece genel (yalnizca tam testte).
    let general_synthesis = general_score.as_ref().map(|gs| synthesize_general(gs, &input));

    Ok(SmartDiagnosis {
        summary,
        issues,
        suggestions,
        general_score,
        source,
        modules,
        overall_status,
        general_synthesis,
    })
}

/// Tek bir modul icin Smart Engine verdict'i (grade + detail_key + insights).
///
/// Smart Engine = TEK grade otoritesi: modul tek basina calistirildiginda da grade
/// bilgisi BURADAN gelir (frontend kendi esigini hesaplamaz -> Smart ile cakisma olmaz).
/// `build_module_verdicts` tum modulleri uretir; istenen `module` id'sine ait olani doner.
/// Bulunamazsa veya o modul bu input'ta olculmediyse `available=false` verdict doner.
#[tauri::command]
pub async fn run_module_verdict(
    module: String,
    input: DiagnosticInput,
) -> Result<ModuleVerdict, String> {
    let verdicts = build_module_verdicts(&input);
    let found = verdicts.into_iter().find(|v| v.module == module);
    match found {
        Some(v) => Ok(v),
        None => Ok(unavailable(&module, "")),
    }
}

/// OYUN buyuk resim sentezi — ping/jitter/bufferbloat/loss kombinasyonu.
/// Yeni FPS motoru (synthesize_fps) bunun yerine kullanılır.
#[allow(dead_code)]
fn synthesize_gaming(gs: &GamingScore) -> Synthesis {
    let mut insights = Vec::new();
    let total = gs.total;
    let grade = score_to_grade(total);

    // En zayif halka (oyun deneyimini bozan ilk faktor)
    let weakest = [
        ("latency", gs.latency), ("jitter", gs.jitter),
        ("bufferbloat", gs.bufferbloat), ("packet_loss", gs.packet_loss),
    ].iter().min_by_key(|(_, v)| *v).map(|(k, _)| *k).unwrap_or("latency");

    // Headline: toplam skora gore
    let headline_key = if total >= 85 {
        "synth_gaming_excellent"
    } else if total >= 70 {
        "synth_gaming_good"
    } else if total >= 55 {
        "synth_gaming_fair"
    } else {
        "synth_gaming_poor"
    }.to_string();

    // Pozitif: guclu faktorler
    if gs.latency >= 85 { insights.push(ins("positive", "synth_g_low_ping")); }
    if gs.jitter >= 85 { insights.push(ins("positive", "synth_g_stable")); }
    if gs.bufferbloat >= 85 { insights.push(ins("positive", "synth_g_no_bloat")); }
    if gs.packet_loss >= 90 { insights.push(ins("positive", "synth_g_no_loss")); }

    // Negatif/notr: en zayif halka uzerinden
    if total < 85 {
        let key = match weakest {
            "latency" => "synth_g_weak_latency",
            "jitter" => "synth_g_weak_jitter",
            "bufferbloat" => "synth_g_weak_bloat",
            _ => "synth_g_weak_loss",
        };
        let kind = if total < 55 { "negative" } else { "neutral" };
        insights.push(ins(kind, key));
    }

    // Sonuc cumlesi (oyun turune gore uygunluk)
    if total >= 85 {
        insights.push(ins("positive", "synth_g_verdict_competitive"));
    } else if total >= 70 {
        insights.push(ins("neutral", "synth_g_verdict_casual"));
    } else {
        insights.push(ins("negative", "synth_g_verdict_struggle"));
    }

    Synthesis { score: total, grade, headline_key, insights }
}

/// GENEL buyuk resim sentezi — hiz/upload/throttle/isp/ipv6/wifi dahil.
fn synthesize_general(gs: &GeneralScore, input: &DiagnosticInput) -> Synthesis {
    let mut insights = Vec::new();
    let total = gs.total;
    let grade = score_to_grade(total);

    let headline_key = if total >= 85 {
        "synth_general_excellent"
    } else if total >= 70 {
        "synth_general_good"
    } else if total >= 55 {
        "synth_general_fair"
    } else {
        "synth_general_poor"
    }.to_string();

    // Hiz (pozitif/negatif)
    if let Some(sp) = gs.speed {
        if sp >= 85 { insights.push(ins("positive", "synth_n_fast")); }
        else if sp < 55 { insights.push(ins("negative", "synth_n_slow")); }
    }
    // Upload dengesi (asimetri)
    let dl = input.speed_download.unwrap_or(0.0);
    let ul = input.speed_upload.unwrap_or(0.0);
    if dl > 0.0 && ul > 0.0 && ul < dl * 0.2 {
        insights.push(ins("neutral", "synth_n_asymmetric"));
    }
    // Bufferbloat (paylasimli kullanim)
    if gs.bufferbloat >= 85 { insights.push(ins("positive", "synth_n_shared_ok")); }
    else if gs.bufferbloat < 55 { insights.push(ins("negative", "synth_n_bloat")); }
    // Throttle
    if let Some(th) = gs.throttle {
        if th >= 95 { insights.push(ins("positive", "synth_n_no_throttle")); }
        else if th < 55 { insights.push(ins("negative", "synth_n_throttle")); }
    }
    // ISP mudahale / guvenlik
    if let Some(isp) = gs.isp_intervention {
        if isp >= 95 { insights.push(ins("positive", "synth_n_clean")); }
        else if isp < 70 { insights.push(ins("negative", "synth_n_interference")); }
    }
    // IPv6 (notr bilgi)
    if let Some(v6) = gs.ipv6 {
        if v6 == 0 { insights.push(ins("neutral", "synth_n_no_ipv6")); }
    }
    // Wi-Fi
    if gs.wifi < 55 { insights.push(ins("neutral", "synth_n_wifi_weak")); }

    Synthesis { score: total, grade, headline_key, insights }
}

/// Skor (0-100) -> 5'li toptan durum etiketi (renk/ozet icin).
fn status_from_score(score: u32) -> &'static str {
    match score {
        85..=100 => "excellent",
        70..=84  => "good",
        55..=69  => "fair",
        35..=54  => "poor",
        _        => "critical",
    }
}

/// Toptan durum: en dusuk skorlu modullere gore (zincirin en zayif halkasi).
fn compute_overall_status(modules: &[ModuleVerdict]) -> String {
    let avail: Vec<&ModuleVerdict> = modules.iter().filter(|m| m.available).collect();
    if avail.is_empty() {
        return "good".to_string();
    }
    // Kritik bir modul varsa toptan da kritik (zayif halka prensibi).
    if avail.iter().any(|m| m.score < 35) {
        return "critical".to_string();
    }
    if avail.iter().any(|m| m.score < 55) {
        return "poor".to_string();
    }
    // Aksi halde ortalamaya gore.
    let avg = avail.iter().map(|m| m.score).sum::<u32>() / avail.len() as u32;
    status_from_score(avg).to_string()
}

/// Eski gaming skor hesaplaması — FPS motoru (compute_fps_score) bunu replace etti.
/// local_analyze hala bu fonksiyonu kullanıyor (summary/issues/suggestions için).
#[allow(dead_code)]
fn compute_gaming_score(input: &DiagnosticInput) -> GamingScore {
    // -------------------------------------------------------------------------
    // GAMING LATENCY (Ağırlık: %35)
    // Kaynak: Riot Games / Valve CS2 sunucu tick rate (128Hz = 7.8ms frame)
    // Kompetitif: <20ms pro seviye, <40ms kabul edilebilir, >80ms oynanamaz
    // Sürekli sürümüş (smooth) — ara değerler lineer interpolasyonla hesaplanır
    // -------------------------------------------------------------------------
    let best_ping = extract_best_ping(&input.ping_results);
    let latency_score: u32 = if best_ping <= 10.0 {
        100 // Pro / LAN seviyesi
    } else if best_ping <= 20.0 {
        // 10-20ms: 100 -> 95
        let t = (best_ping - 10.0) / 10.0;
        (100.0 - t * 5.0) as u32
    } else if best_ping <= 40.0 {
        // 20-40ms: 95 -> 80
        let t = (best_ping - 20.0) / 20.0;
        (95.0 - t * 15.0) as u32
    } else if best_ping <= 60.0 {
        // 40-60ms: 80 -> 60
        let t = (best_ping - 40.0) / 20.0;
        (80.0 - t * 20.0) as u32
    } else if best_ping <= 80.0 {
        // 60-80ms: 60 -> 35
        let t = (best_ping - 60.0) / 20.0;
        (60.0 - t * 25.0) as u32
    } else if best_ping <= 120.0 {
        // 80-120ms: 35 -> 10
        let t = (best_ping - 80.0) / 40.0;
        (35.0 - t * 25.0) as u32
    } else {
        5 // >120ms: kompetitif oyun imkansız
    };

    // -------------------------------------------------------------------------
    // GAMING JITTER (Ağırlık: %30)
    // Kaynak: gaming gecikme rehberleri, endüstri standardı ağ testleri
    // <1ms: profesyonel, <3ms: iyi, <5ms: kabul edilebilir,
    // 5-15ms: hissedilir spike, >15ms: ciddi lag spike
    // -------------------------------------------------------------------------
    let avg_jitter = extract_avg_jitter(&input.ping_results);
    let jitter_score: u32 = if avg_jitter <= 1.0 {
        100
    } else if avg_jitter <= 3.0 {
        let t = (avg_jitter - 1.0) / 2.0;
        (100.0 - t * 10.0) as u32 // 100->90
    } else if avg_jitter <= 5.0 {
        let t = (avg_jitter - 3.0) / 2.0;
        (90.0 - t * 20.0) as u32 // 90->70
    } else if avg_jitter <= 10.0 {
        let t = (avg_jitter - 5.0) / 5.0;
        (70.0 - t * 25.0) as u32 // 70->45
    } else if avg_jitter <= 20.0 {
        let t = (avg_jitter - 10.0) / 10.0;
        (45.0 - t * 30.0) as u32 // 45->15
    } else {
        5 // >20ms: ciddi spike, oyun oynanamaz
    };

    // -------------------------------------------------------------------------
    // GAMING BUFFERBLOAT (Ağırlık: %25)
    // Kaynak: endüstri standardı bufferbloat test metodolojisi
    // Grading: A+(latans artışı <5ms), A(<30ms), B(<60ms), C(<200ms), D(<400ms), F(>=400ms)
    // Oyuncu için kritik: indirirken/yüklerken ping spike’sı = kill kaydı
    // -------------------------------------------------------------------------
    let bloat_grade = input.bufferbloat
        .get("grade")
        .and_then(|v| v.as_str())
        .unwrap_or("F");
    let bufferbloat_score: u32 = match bloat_grade {
        "A+" => 100, // <5ms artış — mükemmel
        "A"  => 88,  // <30ms — iyi
        "B"  => 65,  // <60ms — kabul edilebilir
        "C"  => 38,  // <200ms — sorunlu
        "D"  => 15,  // <400ms — kötü
        _    => 3,   // >=400ms — kullanılamaz
    };

    // -------------------------------------------------------------------------
    // GAMING PACKET LOSS (Ağırlık: %10)
    // Kaynak: Riot Games resmi ağ gereksinimleri, ITU-T G.1010
    // %0: ideal, %0.1: hissedilir, %0.5: ciddi, %1: oyun bozuk, %3: oynanamaz
    // -------------------------------------------------------------------------
    let avg_loss = extract_avg_loss(&input.ping_results);
    let loss_score: u32 = if avg_loss == 0.0 {
        100
    } else if avg_loss <= 0.1 {
        let t = avg_loss / 0.1;
        (100.0 - t * 15.0) as u32 // 100->85
    } else if avg_loss <= 0.5 {
        let t = (avg_loss - 0.1) / 0.4;
        (85.0 - t * 30.0) as u32 // 85->55
    } else if avg_loss <= 1.0 {
        let t = (avg_loss - 0.5) / 0.5;
        (55.0 - t * 25.0) as u32 // 55->30
    } else if avg_loss <= 3.0 {
        let t = (avg_loss - 1.0) / 2.0;
        (30.0 - t * 20.0) as u32 // 30->10
    } else {
        3 // >%3: oynanamaz
    };

    // Gaming ağırlıklı toplam: latency %35 + jitter %30 + bufferbloat %25 + loss %10
    let total = (latency_score * 35
        + jitter_score * 30
        + bufferbloat_score * 25
        + loss_score * 10)
        / 100;

    let grade = score_to_grade(total);

    GamingScore {
        latency: latency_score,
        jitter: jitter_score,
        bufferbloat: bufferbloat_score,
        packet_loss: loss_score,
        total,
        grade,
    }
}

/// Wi-Fi health score from direct dBm RSSI + co-channel neighbor count.
/// Returns 0-100; goes through score_to_grade pipeline when used in ModuleVerdict.
/// Research basis: MetaGeek, Meraki, Revolution Wi-Fi RSSI thresholds;
/// co-channel penalty from Cisco/Aruba enterprise engineering guidance.
fn compute_wifi_health_score(rssi_dbm: i32, co_channel: u32) -> u32 {
    let rssi_score: u32 = if rssi_dbm >= -67 {
        95
    } else if rssi_dbm >= -70 {
        85
    } else if rssi_dbm >= -75 {
        70
    } else if rssi_dbm >= -80 {
        50
    } else {
        25
    };
    let penalty: u32 = match co_channel {
        0        => 0,
        1..=2    => 10,
        3..=4    => 20,
        _        => 30,
    };
    rssi_score.saturating_sub(penalty).clamp(0, 100)
}

/// Extract connected AP RSSI (dBm) and co-channel foreign-AP count from wifi_scan JSON.
/// Returns (rssi_dbm, co_channel_count) or None if scan data unavailable/no connected AP.
const PSC_6GHZ: [u32; 15] = [5, 21, 37, 53, 69, 85, 101, 117, 133, 149, 165, 181, 197, 213, 229];

fn extract_wifi_scan_metrics(wifi_scan: &serde_json::Value) -> Option<(i32, u32, String, String, u32)> {
    let aps = wifi_scan.get("access_points")?.as_array()?;
    // Find connected AP
    let connected = aps.iter().find(|ap| {
        ap.get("is_connected").and_then(|v| v.as_bool()).unwrap_or(false)
    })?;
    let rssi_dbm = connected.get("rssi").and_then(|v| v.as_i64()).unwrap_or(-100) as i32;
    let my_ch = connected.get("channel").and_then(|v| v.as_u64()).unwrap_or(0);
    let my_bssid = connected.get("bssid").and_then(|v| v.as_str()).unwrap_or("");
    let my_oui = if my_bssid.len() >= 8 { &my_bssid[..8] } else { "" };
    let my_ssid = connected.get("ssid").and_then(|v| v.as_str()).unwrap_or("");
    // Count foreign co-channel APs above noise floor
    let co_channel = aps.iter().filter(|ap| {
        let is_conn = ap.get("is_connected").and_then(|v| v.as_bool()).unwrap_or(false);
        if is_conn { return false; }
        let ch = ap.get("channel").and_then(|v| v.as_u64()).unwrap_or(0);
        if ch != my_ch { return false; }
        let bssid = ap.get("bssid").and_then(|v| v.as_str()).unwrap_or("");
        let oui = if bssid.len() >= 8 { &bssid[..8] } else { "" };
        let ssid = ap.get("ssid").and_then(|v| v.as_str()).unwrap_or("");
        // Exclude own OUI or same SSID (same AP/roaming partner)
        if !oui.is_empty() && oui == my_oui { return false; }
        if !ssid.is_empty() && ssid == my_ssid { return false; }
        // Above noise floor (-85 dBm)
        let rssi = ap.get("rssi").and_then(|v| v.as_i64()).unwrap_or(-100);
        rssi > -85
    }).count() as u32;
    let band  = connected.get("band").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let width = connected.get("channel_width").and_then(|v| v.as_str()).unwrap_or("").to_string();
    Some((rssi_dbm, co_channel, band, width, my_ch as u32))
}

fn compute_general_score(input: &DiagnosticInput) -> GeneralScore {
    // =========================================================================
    // GENEL AĞ KALİTESB — Ev kullanıcısı profili
    // Ağırlıklar: latency 12% + jitter 8% + bufferbloat 10% + loss 10%
    //           + speed 25% + wifi 10% + throttle 12% + isp 10% + ipv6 3%
    // Kaynak: FCC Broadband Data 2024, ITU-T Y.1541, kamuya açık Türkiye ortalama verisi Q1-2024,
    //         APNIC IPv6 stats, RFC 2544/3550
    // =========================================================================

    // -------------------------------------------------------------------------
    // LATENCY (12%) — ev kullanıcısı için oyuncu kadar kritik değil
    // Video konferans (Zoom/Teams): <150ms ITU-T G.114
    // Genel browsing: <100ms hissedilmez, <300ms kabul edilebilir
    // -------------------------------------------------------------------------
    let best_ping = extract_best_ping(&input.ping_results);
    let latency_score: u32 = if best_ping <= 20.0 {
        100
    } else if best_ping <= 50.0 {
        let t = (best_ping - 20.0) / 30.0;
        (100.0 - t * 15.0) as u32 // 100->85
    } else if best_ping <= 100.0 {
        let t = (best_ping - 50.0) / 50.0;
        (85.0 - t * 25.0) as u32 // 85->60
    } else if best_ping <= 150.0 {
        let t = (best_ping - 100.0) / 50.0;
        (60.0 - t * 25.0) as u32 // 60->35
    } else if best_ping <= 300.0 {
        let t = (best_ping - 150.0) / 150.0;
        (35.0 - t * 20.0) as u32 // 35->15
    } else {
        5
    };

    // -------------------------------------------------------------------------
    // JITTER (8%) — VoIP/video konferans baz alındı
    // ITU-T G.1010: VoIP için <20ms jitter kabul edilebilir
    // Yüksek jitter jitter-buffer taşmasına yol açar → geç paketler atılır (etkin loss gibi davranır)
    // -------------------------------------------------------------------------
    let avg_jitter = extract_avg_jitter(&input.ping_results);
    let jitter_score: u32 = if avg_jitter <= 2.0 {
        100
    } else if avg_jitter <= 5.0 {
        let t = (avg_jitter - 2.0) / 3.0;
        (100.0 - t * 15.0) as u32 // 100->85
    } else if avg_jitter <= 15.0 {
        let t = (avg_jitter - 5.0) / 10.0;
        (85.0 - t * 30.0) as u32 // 85->55
    } else if avg_jitter <= 30.0 {
        let t = (avg_jitter - 15.0) / 15.0;
        (55.0 - t * 30.0) as u32 // 55->25
    } else if avg_jitter <= 50.0 {
        let t = (avg_jitter - 30.0) / 20.0;
        (25.0 - t * 15.0) as u32 // 25->10
    } else {
        5
    };

    // -------------------------------------------------------------------------
    // BUFFERBLOAT (10%) — ev kullanıcısı: Netflix/YouTube + tarama eş zamanlı
    // Endüstri standardı gecikme grading: A+ <5ms, A <30ms, B <60ms, C <200ms, D <400ms
    // -------------------------------------------------------------------------
    let bloat_grade = input.bufferbloat
        .get("grade")
        .and_then(|v| v.as_str())
        .unwrap_or("F");
    let bufferbloat_score: u32 = match bloat_grade {
        "A+" => 100,
        "A"  => 85,
        "B"  => 65,
        "C"  => 40,
        "D"  => 18,
        _    => 5,
    };

    // -------------------------------------------------------------------------
    // PACKET LOSS (10%) — ITU-T Y.1541 Sınıf 1 (genel internet)
    // Hedef: <%0.1, kabul edilebilir: <%1, sorunlu: >%1
    // HTTP/TCP kendi başlı retransmit yapsa da yüksek kayıp throughput'ı düşürür
    // -------------------------------------------------------------------------
    let avg_loss = extract_avg_loss(&input.ping_results);
    let loss_score: u32 = if avg_loss == 0.0 {
        100
    } else if avg_loss <= 0.1 {
        let t = avg_loss / 0.1;
        (100.0 - t * 10.0) as u32 // 100->90
    } else if avg_loss <= 1.0 {
        let t = (avg_loss - 0.1) / 0.9;
        (90.0 - t * 35.0) as u32 // 90->55
    } else if avg_loss <= 3.0 {
        let t = (avg_loss - 1.0) / 2.0;
        (55.0 - t * 30.0) as u32 // 55->25
    } else if avg_loss <= 10.0 {
        let t = (avg_loss - 3.0) / 7.0;
        (25.0 - t * 15.0) as u32 // 25->10
    } else {
        3
    };

    // -------------------------------------------------------------------------
    // HİZ (25%) — genel skor için en kritik faktör
    // FCC 2024: 100/20 Mbps = broadband minimum
    // Türkiye ortalama: ~75 Mbps (kamuya açık veri Q1-2024)
    // Netflix 4K: 25 Mbps, YouTube 4K: 20 Mbps, Steam ort. DL: 20-50 Mbps
    // Eşikler Türkiye reali tesine göre: 50 Mbps "iyi", 100 Mbps "mükemmel"
    // -------------------------------------------------------------------------
    let speed_score: Option<u32> = input.speed_download.map(|dl| {
        if dl >= 300.0 {
            100
        } else if dl >= 100.0 {
            let t = (dl - 100.0) / 200.0;
            (88.0 + t * 12.0) as u32 // 88->100
        } else if dl >= 50.0 {
            let t = (dl - 50.0) / 50.0;
            (70.0 + t * 18.0) as u32 // 70->88
        } else if dl >= 25.0 {
            let t = (dl - 25.0) / 25.0;
            (50.0 + t * 20.0) as u32 // 50->70
        } else if dl >= 10.0 {
            let t = (dl - 10.0) / 15.0;
            (28.0 + t * 22.0) as u32 // 28->50
        } else if dl >= 4.0 {
            let t = (dl - 4.0) / 6.0;
            (10.0 + t * 18.0) as u32 // 10->28
        } else {
            5 // <4 Mbps: modern kullanım için yetersiz
        }
    });

    // -------------------------------------------------------------------------
    // WI-FI (10%) — IEEE 802.11 RSSI + co-channel yoğunluk
    // Eğer wifi_scan mevcutsa: direct dBm RSSI + co-channel penalty (daha doğru).
    // wifi_scan yoksa: signal_percent tabanlı eski davranış korunur (graceful fallback).
    // Ethernet bağlıysa tam puan (her iki yolda da).
    // -------------------------------------------------------------------------
    let wifi_connected = input.wifi
        .get("connected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let wifi_signal = input.wifi
        .get("signal_percent")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let wifi_score: u32 = if !wifi_connected {
        100 // Ethernet → tam puan
    } else if let Some(scan) = input.wifi_scan.as_ref().and_then(|s| {
        // wifi_scan mevcut: bağlı AP'den direkt dBm + co-channel hesabı
        extract_wifi_scan_metrics(s)
    }) {
        let (rssi_dbm, co_channel, _, _, _) = scan;
        compute_wifi_health_score(rssi_dbm, co_channel)
    } else if wifi_signal >= 80 {
        // ≥-50dBm: mükemmel (graceful fallback: signal_percent)
        let t = (wifi_signal as f64 - 80.0) / 20.0;
        (88.0 + t * 12.0) as u32 // 88->100
    } else if wifi_signal >= 60 {
        // ≈-65dBm: iyi
        let t = (wifi_signal as f64 - 60.0) / 20.0;
        (68.0 + t * 20.0) as u32 // 68->88
    } else if wifi_signal >= 40 {
        // ≈-75dBm: zayıf
        let t = (wifi_signal as f64 - 40.0) / 20.0;
        (38.0 + t * 30.0) as u32 // 38->68
    } else if wifi_signal >= 20 {
        // ≈-85dBm: çok zayıf
        let t = (wifi_signal as f64 - 20.0) / 20.0;
        (15.0 + t * 23.0) as u32 // 15->38
    } else {
        8 // <20%: bağlantı eşiğinde
    };

    // -------------------------------------------------------------------------
    // ISP DARALTMA / THROTTLING (12%)
    // Binary değil — şiddetine göre farklı ceza
    // isp_throttles=false: tam puan
    // isp_throttles=true: kaç protokol etkilendi?
    //   1 protokol (sadece BT): hafif kısıtlama → 55
    //   2 protokol: orta → 30
    //   3 protokol: ağır → 12
    // -------------------------------------------------------------------------
    let throttle_score: Option<u32> = input.throttle.as_ref().map(|t| {
        let throttled = t.get("isp_throttles")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !throttled {
            return 100;
        }
        // Kaç protokol "Throttled" veya "Blocked"? (BT/QUIC/WG/OpenVPN — 4 protokol)
        let bt  = matches!(t.get("bittorrent_verdict").and_then(|v| v.as_str()), Some("Throttled") | Some("Blocked"));
        let qu  = matches!(t.get("quic_verdict").and_then(|v| v.as_str()),       Some("Throttled") | Some("Blocked"));
        let wg  = matches!(t.get("wireguard_verdict").and_then(|v| v.as_str()),  Some("Throttled") | Some("Blocked"));
        let ov  = matches!(t.get("openvpn_verdict").and_then(|v| v.as_str()),    Some("Throttled") | Some("Blocked"));
        match (bt as u8) + (qu as u8) + (wg as u8) + (ov as u8) {
            0 => 100, // isp_throttles=true ama hiçbir protokol match etmedi
            1 => 58,  // hafif (genellikle sadece BT — bazı ISS'ler torrent'i kısıtlar)
            2 => 35,  // orta
            3 => 18,  // ciddi — VPN protokolleri de etkileniyor
            _ => 8,   // ağır — dört protokol birden (agresif DPI)
        }
    });

    // -------------------------------------------------------------------------
    // ISP MÜDAHALE (10%) — şiddete göre diferansiye
    // DNS Hijacking: önemi var ama kolay aşılır (DoH kullan) → hafif ceza
    // Transparent Proxy: orta — performansı düşürebilir
    // SNI Filtering: orta-ağır — bazı sitelere erişim engeli
    // MITM: çok ağır — güvenlik ihlali
    // Türkiye'de DNS hijacking yaygın — tek başına 30 puan düşmüyor
    // -------------------------------------------------------------------------
    let isp_score: Option<u32> = input.isp_intervention.as_ref().map(|isp| {
        let dns     = matches!(isp.get("dns_hijack").and_then(|v| v.get("verdict")).and_then(|v| v.as_str()),      Some("Hijacked"));
        let proxy   = matches!(isp.get("transparent_proxy").and_then(|v| v.get("verdict")).and_then(|v| v.as_str()), Some("Detected"));
        let sni     = matches!(isp.get("sni_filtering").and_then(|v| v.get("verdict")).and_then(|v| v.as_str()),   Some("Filtered"));
        let mitm    = matches!(isp.get("mitm").and_then(|v| v.get("verdict")).and_then(|v| v.as_str()),            Some("Detected"));

        // Ceza puanları (kumulatif, min 5)
        let mut penalty: u32 = 0;
        if dns   { penalty += 12; } // Yaygın, hafif
        if proxy { penalty += 20; } // Performans etkisi
        if sni   { penalty += 25; } // Erişim engeli
        if mitm  { penalty += 40; } // Güvenlik ihlali — ağır
        100u32.saturating_sub(penalty).max(5)
    });

    // -------------------------------------------------------------------------
    // IPv6 (3%) — düşük ağırlık: Türkiye IPv6 penetrasyon ~%8 (APNIC 2024)
    // Yoksa 0 — mantıklı çünkü IPv6 desteği ek bir kalite göstergesi
    // Partial: ISP tunnel/6to4 → native kadar değerli değil
    // -------------------------------------------------------------------------
    let ipv6_score: Option<u32> = input.ipv6.as_ref().map(|v6| {
        let grade = v6
            .get("connectivity")
            .and_then(|c| c.get("connectivity_grade"))
            .and_then(|g| g.as_str())
            .unwrap_or("None");
        match grade {
            "Full"    => 100, // Native dual-stack
            "Partial" => 45,  // Tunnel/6to4 — tam native değil
            _         => 0,   // IPv6 yok — 0 puan (haklı talep)
        }
    });

    // -------------------------------------------------------------------------
    // DİNAMİK AĞIRLIKLI TOPLAM
    // Sabit (her zaman ölçülür): latency(12)+jitter(8)+bufferbloat(10)+loss(10)+wifi(10)=50
    // Opsiyonel: speed(25)+throttle(12)+isp(10)+ipv6(3) = max 50
    // -------------------------------------------------------------------------
    let mut weighted_sum: u32 = latency_score * 12
        + jitter_score * 8
        + bufferbloat_score * 10
        + loss_score * 10
        + wifi_score * 10;
    let mut total_weight: u32 = 50;

    // CGNAT skoru (dusuk agirlik): teoride seffaf ama pratikte port forward yok,
    // P2P/oyun NAT sorunlari, cift-NAT gecikmesi -> gercek kullanici acisi.
    // 'unknown' -> None (olculemedi, skora katma; UPnP kapaliysa).
    let cgnat_score: Option<u32> = input.cgnat.as_ref().and_then(|c| {
        match c.get("verdict").and_then(|v| v.as_str()) {
            Some("public")     => Some(100),
            Some("double_nat") => Some(50),
            Some("cgnat")      => Some(30),
            _                  => None, // unknown/eksik
        }
    });

    if let Some(s) = speed_score    { weighted_sum += s * 25; total_weight += 25; }
    if let Some(s) = throttle_score { weighted_sum += s * 12; total_weight += 12; }
    if let Some(s) = isp_score      { weighted_sum += s * 10; total_weight += 10; }
    if let Some(s) = ipv6_score     { weighted_sum += s * 3;  total_weight += 3;  }
    if let Some(s) = cgnat_score    { weighted_sum += s * 4;  total_weight += 4;  }

    // MTU skoru (dusuk agirlik): cogu MTU normal (1500/1492 PPPoE), skoru etkilemez.
    // Yalnizca GERCEKTEN sorunlu (dusuk/kritik) MTU low grade dondurur. None=olculmedi.
    let mtu_score: Option<u32> = input.mtu.as_ref().and_then(|m| {
        m.get("primary_mtu").and_then(|v| v.as_u64()).map(|mtu| {
            if mtu >= 1492 { 100 }       // optimal / PPPoE (normal)
            else if mtu >= 1400 { 85 }   // tunel/VPN overhead (kabul edilebilir)
            else if mtu >= 1280 { 55 }   // dusuk — fragmentasyon riski
            else { 25 }                  // <1280 IPv6 min alti, ciddi
        })
    });
    if let Some(s) = mtu_score      { weighted_sum += s * 3;  total_weight += 3;  }

    // VoIP skoru (dusuk agirlik=3): E-model MOS -> 0-100 dogrusal donusum.
    // mos=None (error/testsiz) -> None -> skora katilmaz (graceful).
    let voip_score: Option<u32> = input.voip.as_ref().and_then(|v| {
        let mos = v.get("voice")?.get("mos")?.as_f64()?;
        Some(((mos - 1.0) / 3.5 * 100.0).clamp(0.0, 100.0) as u32)
    });
    if let Some(s) = voip_score     { weighted_sum += s * 3;  total_weight += 3;  }

    // DNS Benchmark skoru: en iyi resolver'in (rank=1) median latency'sine gore.
    // NOT: Genel agirlikli toplama KATILMAZ (DNS herkes calistirmaz, genel skoru bozmasin).
    // Yalnizca modul-bazli grade icin hesaplanir (build_module_verdicts kullanir).
    let dns_score: Option<u32> = input.dns_benchmark.as_ref().and_then(|d| {
        let providers = d.get("providers")?.as_array()?;
        let best = providers.iter().find(|p| p.get("rank").and_then(|r| r.as_u64()) == Some(1))
            .or_else(|| providers.first())?;
        let ms = best.get("best_ms")?.as_f64()?;
        Some(dns_latency_score(ms))
    });

    // Twitch skoru: en yakin (en dusuk RTT) ingest sunucusuna gore.
    // Genel toplama KATILMAZ (nis ozellik) — yalnizca modul grade'i icin.
    let twitch_score: Option<u32> = input.twitch.as_ref().and_then(|tw| {
        let servers = tw.get("servers")?.as_array()?;
        let best_rtt = servers.iter()
            .filter_map(|s| s.get("rtt_ms").and_then(|v| v.as_f64()))
            .fold(f64::INFINITY, f64::min);
        if best_rtt.is_finite() { Some(twitch_rtt_score(best_rtt)) } else { None }
    });

    let total = weighted_sum / total_weight;
    let grade = score_to_grade(total);

    GeneralScore {
        latency: latency_score,
        jitter: jitter_score,
        bufferbloat: bufferbloat_score,
        packet_loss: loss_score,
        speed: speed_score,
        wifi: wifi_score,
        throttle: throttle_score,
        isp_intervention: isp_score,
        ipv6: ipv6_score,
        cgnat: cgnat_score,
        mtu: mtu_score,
        voip: voip_score,
        dns_benchmark: dns_score,
        twitch: twitch_score,
        total,
        grade,
    }
}

/// DNS resolver median latency (ms) -> 0-100 skor. score_to_grade ile harfe cevrilir.
/// Esikler: yerel/CDN resolver pratigi (Cloudflare/Google tipik <20ms).
fn dns_latency_score(ms: f64) -> u32 {
    if ms <= 10.0 { 100 }       // mukemmel (yerel/yakin cache)
    else if ms <= 20.0 { 90 }   // cok iyi (Cloudflare/Google tipik)
    else if ms <= 40.0 { 78 }   // iyi
    else if ms <= 70.0 { 68 }   // orta
    else if ms <= 120.0 { 55 }  // zayif
    else { 30 }                 // kotu (uzak/yavas resolver)
}

/// Twitch ingest RTT (en yakin sunucu, ms) -> 0-100 skor.
/// Esikler: yayincilik icin RTT toleransi (RTMP/TCP ingest).
fn twitch_rtt_score(ms: f64) -> u32 {
    if ms <= 20.0 { 100 }       // mukemmel (yakin ingest)
    else if ms <= 40.0 { 88 }   // cok iyi
    else if ms <= 70.0 { 78 }   // iyi
    else if ms <= 120.0 { 66 }  // orta
    else if ms <= 200.0 { 52 }  // zayif
    else { 25 }                 // kotu (uzak ingest)
}

// =============================================================================
// SMART DIAGNOSIS ENGINE — modul-modul yapilandirilmis yorumlar (Faz 3)
// Tek skor kaynagi: compute_general_score (opsiyonel moduller None doner).
// Her modul: ham metrik + skor -> grade -> i18n detail_key.
// Backend metin uretmez; frontend t(detail_key, {metric}) ile cevirir.
// =============================================================================
fn build_module_verdicts(input: &DiagnosticInput) -> Vec<ModuleVerdict> {
    let gs = compute_general_score(input);
    let mut out = Vec::new();

    // ── Cekirdek (her zaman olculur) ────────────────────────────────────
    let best_ping = extract_best_ping(&input.ping_results);
    let mut latency_v = verdict("latency", gs.latency, best_ping, "ms", true);
    latency_v.insights = latency_insights(best_ping);
    out.push(latency_v);

    let avg_jitter = extract_avg_jitter(&input.ping_results);
    let wifi_connected = input.wifi.get("connected").and_then(|v| v.as_bool()).unwrap_or(false);
    let wifi_signal = input.wifi.get("signal_percent").and_then(|v| v.as_u64()).unwrap_or(0) as f64;
    let mut jitter_v = verdict("jitter", gs.jitter, avg_jitter, "ms", true);
    jitter_v.insights = jitter_insights(avg_jitter, wifi_connected);
    out.push(jitter_v);

    let avg_loss = extract_avg_loss(&input.ping_results);
    let mut loss_v = verdict("loss", gs.packet_loss, avg_loss, "%", true);
    loss_v.insights = loss_insights(avg_loss);
    out.push(loss_v);

    let bloat_delta = input.bufferbloat.get("latency_delta_ms")
        .and_then(|v| v.as_f64()).unwrap_or(0.0);
    let mut bloat_v = verdict("bufferbloat", gs.bufferbloat, bloat_delta, "ms", true);
    bloat_v.insights = bufferbloat_insights(bloat_delta);
    out.push(bloat_v);

    // wifi_insights ek parametreler: rssi_dbm + co_channel + band + width + ch (wifi_scan'dan)
    let (wifi_rssi_dbm, wifi_co_channel, wifi_band, wifi_width, wifi_ch) = input.wifi_scan.as_ref()
        .and_then(extract_wifi_scan_metrics)
        .unwrap_or_else(|| {
            let dbm = input.wifi.get("signal_dbm").and_then(|v| v.as_i64()).unwrap_or(-100) as i32;
            (dbm, 0u32, String::new(), String::new(), 0u32)
        });
    let mut wifi_v = verdict("wifi", gs.wifi, if wifi_connected { wifi_signal } else { 100.0 }, "%", true);
    wifi_v.insights = wifi_insights(wifi_connected, wifi_signal, wifi_rssi_dbm, wifi_co_channel, &wifi_band, &wifi_width, wifi_ch);
    out.push(wifi_v);

    // ── Opsiyonel (yalnizca tam testte) ────────────────────────────────
    match gs.speed {
        Some(s) => {
            let dl = input.speed_download.unwrap_or(0.0);
            let ul = input.speed_upload.unwrap_or(0.0);
            let mut v = verdict("speed", s, dl, "Mbps", true);
            v.insights = speed_insights(dl, ul);
            out.push(v);
        }
        None => out.push(unavailable("speed", "Mbps")),
    }
    match gs.throttle {
        Some(s) => {
            let mut v = verdict("throttle", s, 0.0, "", true);
            v.insights = throttle_insights(input);
            out.push(v);
        }
        None => out.push(unavailable("throttle", "")),
    }
    match gs.isp_intervention {
        Some(s) => {
            let mut v = verdict("isp_intervention", s, 0.0, "", true);
            v.insights = isp_insights(input);
            out.push(v);
        }
        None => out.push(unavailable("isp_intervention", "")),
    }
    match gs.ipv6 {
        Some(s) => {
            let mut v = verdict("ipv6", s, 0.0, "", true);
            v.insights = ipv6_insights(input);
            out.push(v);
        }
        None => out.push(unavailable("ipv6", "")),
    }
    match gs.cgnat {
        Some(s) => {
            let mut v = verdict("cgnat", s, 0.0, "", true);
            v.insights = cgnat_insights(input);
            out.push(v);
        }
        None => out.push(unavailable("cgnat", "")),
    }
    match gs.mtu {
        Some(s) => {
            // metric: olculen MTU (byte) — frontend gosterebilir
            let mtu_val = input.mtu.as_ref()
                .and_then(|m| m.get("primary_mtu")).and_then(|v| v.as_u64())
                .unwrap_or(0) as f64;
            let mut v = verdict("mtu", s, mtu_val, "B", true);
            v.insights = mtu_insights(input);
            out.push(v);
        }
        None => out.push(unavailable("mtu", "B")),
    }
    match gs.voip {
        Some(s) => {
            let mos_val = input.voip.as_ref()
                .and_then(|v| v.get("voice")).and_then(|v| v.get("mos"))
                .and_then(|v| v.as_f64()).unwrap_or(0.0);
            let mut v = verdict("voip", s, mos_val, "MOS", true);
            v.insights = voip_insights(input);
            out.push(v);
        }
        None => out.push(unavailable("voip", "MOS")),
    }
    match gs.dns_benchmark {
        Some(s) => {
            // metric: en iyi resolver median latency (ms)
            let best_ms = input.dns_benchmark.as_ref()
                .and_then(|d| d.get("providers")).and_then(|p| p.as_array())
                .and_then(|arr| arr.iter().find(|p| p.get("rank").and_then(|r| r.as_u64()) == Some(1)).or_else(|| arr.first()))
                .and_then(|p| p.get("best_ms")).and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let mut v = verdict("dns_benchmark", s, best_ms, "ms", true);
            v.insights = dns_insights(best_ms);
            out.push(v);
        }
        None => out.push(unavailable("dns_benchmark", "ms")),
    }
    match gs.twitch {
        Some(s) => {
            let best_rtt = input.twitch.as_ref()
                .and_then(|tw| tw.get("servers")).and_then(|sv| sv.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.get("rtt_ms").and_then(|v| v.as_f64())).fold(f64::INFINITY, f64::min))
                .filter(|r| r.is_finite()).unwrap_or(0.0);
            let mut v = verdict("twitch", s, best_rtt, "ms", true);
            v.insights = twitch_insights(best_rtt);
            out.push(v);
        }
        None => out.push(unavailable("twitch", "ms")),
    }

    out
}

/// DNS Benchmark icgoruleri (en iyi resolver latency'sine gore, basit Faz 1 seti).
fn dns_insights(best_ms: f64) -> Vec<Insight> {
    let mut v = Vec::new();
    if best_ms <= 20.0 {
        v.push(Insight { kind: "positive".into(), text_key: "insight_dns_fast".into() });
    } else if best_ms <= 70.0 {
        v.push(Insight { kind: "neutral".into(), text_key: "insight_dns_moderate".into() });
    } else {
        v.push(Insight { kind: "negative".into(), text_key: "insight_dns_slow".into() });
    }
    v
}

/// Twitch ingest icgoruleri (en yakin sunucu RTT'sine gore, basit Faz 1 seti).
fn twitch_insights(best_rtt: f64) -> Vec<Insight> {
    let mut v = Vec::new();
    if best_rtt <= 40.0 {
        v.push(Insight { kind: "positive".into(), text_key: "insight_twitch_low".into() });
    } else if best_rtt <= 120.0 {
        v.push(Insight { kind: "neutral".into(), text_key: "insight_twitch_moderate".into() });
    } else {
        v.push(Insight { kind: "negative".into(), text_key: "insight_twitch_high".into() });
    }
    v
}

/// Olculen bir modul icin verdict olusturur (skor -> grade -> detail_key).
fn verdict(module: &str, score: u32, metric: f64, unit: &str, available: bool) -> ModuleVerdict {
    let grade = score_to_grade(score);
    // detail_key: "diag_<module>_<grade-sade>" — frontend cevirir.
    // A+ -> "Aplus" (anahtar guvenli karakter).
    let grade_key = match grade.as_str() {
        "A+" => "Aplus",
        g => g,
    };
    let detail_key = format!("diag_{}_{}", module, grade_key);
    ModuleVerdict {
        module: module.to_string(),
        score,
        grade,
        metric: (metric * 10.0).round() / 10.0,
        metric_unit: unit.to_string(),
        detail_key,
        available,
        insights: Vec::new(),
    }
}

fn ins(kind: &str, key: &str) -> Insight {
    Insight { kind: kind.to_string(), text_key: key.to_string() }
}

/// LATENCY icgoruleri (Faz 3b pilot) — best_ping (ms) esiklerine gore.
/// Esikler compute_*_score latency mantigi ile uyumlu (ITU-T G.114, gaming pratigi).
fn latency_insights(ms: f64) -> Vec<Insight> {
    let mut v = Vec::new();
    if ms <= 20.0 {
        v.push(ins("positive", "insight_latency_competitive"));
        v.push(ins("positive", "insight_latency_realtime"));
    } else if ms <= 40.0 {
        v.push(ins("positive", "insight_latency_good_gaming"));
        v.push(ins("positive", "insight_latency_realtime"));
    } else if ms <= 80.0 {
        v.push(ins("positive", "insight_latency_casual_ok"));
    }
    if ms > 40.0 && ms <= 80.0 {
        v.push(ins("neutral", "insight_latency_fps_disadvantage"));
    } else if ms > 80.0 && ms <= 150.0 {
        v.push(ins("neutral", "insight_latency_browsing_ok"));
    }
    if ms > 80.0 && ms <= 150.0 {
        v.push(ins("negative", "insight_latency_gaming_laggy"));
        v.push(ins("negative", "insight_latency_closer_server"));
    } else if ms > 150.0 {
        v.push(ins("negative", "insight_latency_realtime_unusable"));
        v.push(ins("negative", "insight_latency_check_route"));
    }
    v
}

/// JITTER icgoruleri (RFC 3550). wifi: Wi-Fi'ye baglıysa oneri farkli.
fn jitter_insights(ms: f64, wifi: bool) -> Vec<Insight> {
    let mut v = Vec::new();
    if ms <= 3.0 {
        v.push(ins("positive", "insight_jitter_stable"));
        v.push(ins("positive", "insight_jitter_voip_ok"));
    } else if ms <= 10.0 {
        v.push(ins("positive", "insight_jitter_acceptable"));
        v.push(ins("neutral", "insight_jitter_minor_spikes"));
    } else if ms <= 20.0 {
        v.push(ins("neutral", "insight_jitter_noticeable"));
        v.push(ins("negative", if wifi { "insight_jitter_use_ethernet" } else { "insight_jitter_check_qos" }));
    } else {
        v.push(ins("negative", "insight_jitter_severe"));
        v.push(ins("negative", if wifi { "insight_jitter_use_ethernet" } else { "insight_jitter_check_qos" }));
    }
    v
}

/// PACKET LOSS icgoruleri (ITU-T G.1010, Riot Games).
fn loss_insights(pct: f64) -> Vec<Insight> {
    let mut v = Vec::new();
    if pct == 0.0 {
        v.push(ins("positive", "insight_loss_none"));
    } else if pct <= 0.5 {
        v.push(ins("positive", "insight_loss_negligible"));
    } else if pct <= 1.0 {
        v.push(ins("neutral", "insight_loss_minor"));
    } else if pct <= 3.0 {
        v.push(ins("negative", "insight_loss_disruptive"));
        v.push(ins("negative", "insight_loss_check_link"));
    } else {
        v.push(ins("negative", "insight_loss_severe"));
        v.push(ins("negative", "insight_loss_contact_isp"));
    }
    v
}

/// BUFFERBLOAT icgoruleri (endüstri standardı metodoloji, delta ms).
fn bufferbloat_insights(delta: f64) -> Vec<Insight> {
    let mut v = Vec::new();
    if delta <= 30.0 {
        v.push(ins("positive", "insight_bloat_low"));
        v.push(ins("positive", "insight_bloat_shared_ok"));
    } else if delta <= 60.0 {
        v.push(ins("positive", "insight_bloat_mild_ok"));
        v.push(ins("neutral", "insight_bloat_heavy_upload"));
    } else if delta <= 150.0 {
        v.push(ins("neutral", "insight_bloat_noticeable"));
        v.push(ins("negative", "insight_bloat_enable_sqm"));
    } else {
        v.push(ins("negative", "insight_bloat_severe"));
        v.push(ins("negative", "insight_bloat_enable_sqm"));
    }
    v
}

/// WI-FI icgoruleri (IEEE 802.11 RSSI + co-channel + band).
/// connected=false ise Ethernet. rssi_dbm ve co_channel wifi_scan'dan gelir;
/// wifi_scan yoksa rssi_dbm=-100 / co_channel=0 / band="" gecilir (graceful).
fn wifi_insights(connected: bool, signal: f64, rssi_dbm: i32, co_channel: u32, band: &str, width: &str, channel: u32) -> Vec<Insight> {
    let mut v = Vec::new();
    if !connected {
        v.push(ins("positive", "insight_wifi_ethernet"));
        return v;
    }
    // RSSI tabanlı temel insight
    if signal >= 70.0 {
        v.push(ins("positive", "insight_wifi_strong"));
    } else if signal >= 45.0 {
        v.push(ins("neutral", "insight_wifi_fair"));
        v.push(ins("neutral", "insight_wifi_move_closer"));
    } else {
        v.push(ins("negative", "insight_wifi_weak"));
        v.push(ins("negative", "insight_wifi_reposition"));
    }
    // Co-channel insight (wifi_scan mevcutsa anlamı olur; yoksa rssi_dbm=-100 olur)
    // rssi_dbm != -100 = wifi_scan verisi var demek (fallback -100 gece girmez)
    if rssi_dbm > -100 {
        match co_channel {
            0 => v.push(ins("positive", "insight_wifi_co_channel_clean")),
            1..=2 => v.push(ins("neutral", "insight_wifi_co_channel_moderate")),
            _ => v.push(ins("negative", "insight_wifi_co_channel_heavy")),
        }
        // Band upgrade hint for 2.4 GHz
        if band == "2.4 GHz" {
            v.push(ins("neutral", "insight_wifi_band_upgrade_hint"));
        }
        // Madde 2: yoğun ortamda geniş kanal
        if co_channel >= 3 && (width.starts_with("80") || width.starts_with("160") || width.starts_with("320")) {
            v.push(ins("neutral", "insight_wifi_width_dense"));
        }
        // Madde 3: 6GHz non-PSC
        if band == "6 GHz" && !PSC_6GHZ.contains(&channel) {
            v.push(ins("neutral", "insight_wifi_6ghz_non_psc"));
        }
        // Passive scan duruştuk notu — her zaman en sona (dürüstlük gereksinimleri)
        v.push(ins("neutral", "insight_wifi_passive_scan_note"));
    }
    v
}

/// SPEED icgoruleri (FCC 2024, kamuya açık ortalama veri). DL + UL Mbps.
fn speed_insights(dl: f64, ul: f64) -> Vec<Insight> {
    let mut v = Vec::new();
    if dl >= 100.0 {
        v.push(ins("positive", "insight_speed_fast"));
    } else if dl >= 50.0 {
        v.push(ins("positive", "insight_speed_good"));
    } else if dl >= 25.0 {
        v.push(ins("neutral", "insight_speed_moderate"));
    } else {
        v.push(ins("negative", "insight_speed_low"));
    }
    // Upload degerlendirmesi (asimetrik hat tespiti)
    if ul > 0.0 && dl > 0.0 {
        if ul < 10.0 {
            v.push(ins("negative", "insight_speed_upload_low"));
        } else if ul < dl * 0.2 {
            v.push(ins("neutral", "insight_speed_asymmetric"));
        } else {
            v.push(ins("positive", "insight_speed_upload_ok"));
        }
    }
    v
}

/// THROTTLE icgoruleri — throttle JSON'undan verdict'lere gore.
fn throttle_insights(input: &DiagnosticInput) -> Vec<Insight> {
    let mut v = Vec::new();
    let t = match &input.throttle { Some(t) => t, None => return v };
    let vd = |k: &str| t.get(k).and_then(|x| x.as_str()).unwrap_or("");
    let bad = |s: &str| s == "Throttled" || s == "Blocked";
    let bt = bad(vd("bittorrent_verdict"));
    let qu = bad(vd("quic_verdict"));
    let wg = bad(vd("wireguard_verdict"));
    let ov = bad(vd("openvpn_verdict"));
    let count = (bt as u8) + (qu as u8) + (wg as u8) + (ov as u8);
    if count == 0 {
        v.push(ins("positive", "insight_throttle_none"));
        v.push(ins("positive", "insight_throttle_protocols_free"));
    } else {
        if bt { v.push(ins("negative", "insight_throttle_bittorrent")); }
        if qu { v.push(ins("negative", "insight_throttle_quic")); }
        if wg { v.push(ins("negative", "insight_throttle_wireguard")); }
        if ov { v.push(ins("negative", "insight_throttle_openvpn")); }
        if count >= 2 {
            v.push(ins("negative", "insight_throttle_vpn_advised"));
        } else {
            v.push(ins("neutral", "insight_throttle_single_protocol"));
        }
    }
    v
}

/// ISP MUDAHALE icgoruleri — isp_intervention JSON'undan.
fn isp_insights(input: &DiagnosticInput) -> Vec<Insight> {
    let mut v = Vec::new();
    let isp = match &input.isp_intervention { Some(i) => i, None => return v };
    let verd = |k: &str| isp.get(k).and_then(|x| x.get("verdict")).and_then(|x| x.as_str()).unwrap_or("");
    let dns = verd("dns_hijack") == "Hijacked";
    let proxy = verd("transparent_proxy") == "Detected";
    let sni = verd("sni_filtering") == "Filtered";
    let mitm = verd("mitm") == "Detected";
    if !dns && !proxy && !sni && !mitm {
        v.push(ins("positive", "insight_isp_clean"));
        v.push(ins("positive", "insight_isp_integrity"));
    } else {
        if dns { v.push(ins("neutral", "insight_isp_dns_hijack")); }
        if proxy { v.push(ins("negative", "insight_isp_proxy")); }
        if sni { v.push(ins("negative", "insight_isp_sni")); }
        if mitm { v.push(ins("negative", "insight_isp_mitm")); }
    }
    v
}

/// IPv6 icgoruleri — ipv6 JSON connectivity_grade'ine gore.
fn ipv6_insights(input: &DiagnosticInput) -> Vec<Insight> {
    let mut v = Vec::new();
    let v6 = match &input.ipv6 { Some(i) => i, None => return v };
    let grade = v6.get("connectivity").and_then(|c| c.get("connectivity_grade"))
        .and_then(|g| g.as_str()).unwrap_or("None");
    match grade {
        "Full" => {
            v.push(ins("positive", "insight_ipv6_native"));
            v.push(ins("positive", "insight_ipv6_futureproof"));
        }
        "Tunnel" | "Partial" => {
            v.push(ins("neutral", "insight_ipv6_partial"));
        }
        _ => {
            v.push(ins("neutral", "insight_ipv6_none"));
        }
    }
    v
}

/// CGNAT icgoruleri — cgnat JSON verdict'ine gore.
fn cgnat_insights(input: &DiagnosticInput) -> Vec<Insight> {
    let mut v = Vec::new();
    let c = match &input.cgnat { Some(c) => c, None => return v };
    match c.get("verdict").and_then(|x| x.as_str()) {
        Some("public") => {
            v.push(ins("positive", "insight_cgnat_public"));
        }
        Some("double_nat") => {
            v.push(ins("negative", "insight_cgnat_double_nat"));
        }
        Some("cgnat") => {
            v.push(ins("negative", "insight_cgnat_detected"));
            v.push(ins("neutral", "insight_cgnat_workaround"));
        }
        _ => {
            v.push(ins("neutral", "insight_cgnat_unknown"));
        }
    }
    v
}

/// MTU icgoruleri — mtu JSON primary_mtu degerine gore.
fn mtu_insights(input: &DiagnosticInput) -> Vec<Insight> {
    let mut v = Vec::new();
    let m = match &input.mtu { Some(m) => m, None => return v };
    let mtu = m.get("primary_mtu").and_then(|x| x.as_u64()).unwrap_or(0);
    if mtu >= 1492 {
        v.push(ins("positive", "insight_mtu_optimal"));
    } else if mtu >= 1400 {
        v.push(ins("neutral", "insight_mtu_tunnel"));
    } else if mtu >= 1280 {
        v.push(ins("negative", "insight_mtu_low"));
    } else if mtu > 0 {
        v.push(ins("negative", "insight_mtu_critical"));
    }
    v
}

/// VoIP icgoruleri — E-model MOS degerine gore.
fn voip_insights(input: &DiagnosticInput) -> Vec<Insight> {
    let mut v = Vec::new();
    let vi = match &input.voip { Some(i) => i, None => return v };
    let mos = vi.get("voice").and_then(|x| x.get("mos")).and_then(|x| x.as_f64()).unwrap_or(0.0);
    if mos >= 4.0      { v.push(ins("positive", "insight_voip_excellent")); }
    else if mos >= 3.6 { v.push(ins("positive", "insight_voip_good")); }
    else if mos >= 3.1 { v.push(ins("neutral",  "insight_voip_acceptable")); }
    else if mos > 0.0  { v.push(ins("negative", "insight_voip_poor")); }
    v.push(ins("neutral", "insight_voip_udp_real"));
    v.push(ins("neutral", "insight_voip_emodel_estimate"));
    v
}

/// Bu testte olculmemis modul (gri/pasif gosterilir).
fn unavailable(module: &str, unit: &str) -> ModuleVerdict {
    ModuleVerdict {
        module: module.to_string(),
        score: 0,
        grade: "-".to_string(),
        metric: 0.0,
        metric_unit: unit.to_string(),
        detail_key: format!("diag_{}_unavailable", module),
        available: false,
        insights: Vec::new(),
    }
}

// =============================================================================
// FPS DIAGNOSIS ENGINE
// Oyuncu odakli, 5 bilesenli skor: latency/jitter/loss/bufferbloat/wifi
// Esikler rekabetci FPS (CS2/Valorant 128-tick) gereksinimlerine gore.
// =============================================================================

fn compute_fps_score(input: &DiagnosticInput) -> FpsScore {
    // ── LATENCY (Agirlik %30) — FPS icin cok siki esikler (128-tick = 7.8ms frame)
    let best_ping = extract_best_ping(&input.ping_results);
    let latency_score: u32 = if best_ping <= 10.0 {
        100
    } else if best_ping <= 20.0 {
        let t = (best_ping - 10.0) / 10.0;
        (100.0 - t * 8.0) as u32   // 100->92
    } else if best_ping <= 30.0 {
        let t = (best_ping - 20.0) / 10.0;
        (92.0 - t * 14.0) as u32   // 92->78
    } else if best_ping <= 50.0 {
        let t = (best_ping - 30.0) / 20.0;
        (78.0 - t * 23.0) as u32   // 78->55
    } else if best_ping <= 70.0 {
        let t = (best_ping - 50.0) / 20.0;
        (55.0 - t * 20.0) as u32   // 55->35
    } else if best_ping <= 100.0 {
        let t = (best_ping - 70.0) / 30.0;
        (35.0 - t * 17.0) as u32   // 35->18
    } else {
        5 // >100ms: rekabetci FPS icin kullanissiz
    };

    // ── JITTER (Agirlik %30) — spike = hit-reg kaybi
    let avg_jitter = extract_avg_jitter(&input.ping_results);
    let jitter_score: u32 = if avg_jitter <= 1.0 {
        100
    } else if avg_jitter <= 2.0 {
        let t = (avg_jitter - 1.0) / 1.0;
        (100.0 - t * 12.0) as u32  // 100->88
    } else if avg_jitter <= 4.0 {
        let t = (avg_jitter - 2.0) / 2.0;
        (88.0 - t * 26.0) as u32   // 88->62
    } else if avg_jitter <= 8.0 {
        let t = (avg_jitter - 4.0) / 4.0;
        (62.0 - t * 24.0) as u32   // 62->38
    } else if avg_jitter <= 15.0 {
        let t = (avg_jitter - 8.0) / 7.0;
        (38.0 - t * 23.0) as u32   // 38->15
    } else {
        5 // >15ms: ciddi spike
    };

    // ── PACKET LOSS (Agirlik %25) — FPS'te %0.1 bile hissedilir
    let avg_loss = extract_avg_loss(&input.ping_results);
    let loss_score: u32 = if avg_loss == 0.0 {
        100
    } else if avg_loss <= 0.1 {
        let t = avg_loss / 0.1;
        (100.0 - t * 20.0) as u32  // 100->80
    } else if avg_loss <= 0.5 {
        let t = (avg_loss - 0.1) / 0.4;
        (80.0 - t * 35.0) as u32   // 80->45
    } else if avg_loss <= 1.0 {
        let t = (avg_loss - 0.5) / 0.5;
        (45.0 - t * 25.0) as u32   // 45->20
    } else {
        5 // >%1: FPS bozulmus
    };

    // ── BUFFERBLOAT (Agirlik %10) — grade-bazli (A+->100 ... F->5)
    let bloat_grade = input.bufferbloat
        .get("grade")
        .and_then(|v| v.as_str())
        .unwrap_or("F");
    let bufferbloat_score: u32 = match bloat_grade {
        "A+" => 100,
        "A"  => 78,
        "B"  => 48,
        "C"  => 22,
        _    => 5,  // D, F veya diger
    };

    // ── WIFI (Agirlik %5) — Ethernet ise en iyi skor (100)
    let wifi_connected = input.wifi
        .get("connected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let wifi_score: u32 = if !wifi_connected {
        // Ethernet -> en iyi (kablolu agde WiFi dezavantaji yok)
        100
    } else if let Some(scan_metrics) = input.wifi_scan.as_ref().and_then(|s| {
        extract_wifi_scan_metrics(s)
    }) {
        // wifi_scan mevcut: direkt RSSI bazli health skoru
        let (rssi_dbm, co_channel, _, _, _) = scan_metrics;
        let raw: f64 = compute_wifi_health_score(rssi_dbm, co_channel) as f64;
        ((raw - 25.0_f64) * 1.33_f64).clamp(0.0_f64, 100.0_f64) as u32
    } else {
        // wifi_scan yok ama WiFi bagli: signal_percent'ten tahmin
        let signal: f64 = input.wifi
            .get("signal_percent")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as f64;
        let raw: f64 = if signal >= 80.0 { 90.0 }
            else if signal >= 60.0 { 70.0 }
            else if signal >= 40.0 { 50.0 }
            else { 25.0 };
        ((raw - 25.0_f64) * 1.33_f64).clamp(0.0_f64, 100.0_f64) as u32
    };

    // Agirlikli toplam: latency*30 + jitter*30 + loss*25 + bufferbloat*10 + wifi*5
    let total = (latency_score.wrapping_mul(30)
        .wrapping_add(jitter_score.wrapping_mul(30))
        .wrapping_add(loss_score.wrapping_mul(25))
        .wrapping_add(bufferbloat_score.wrapping_mul(10))
        .wrapping_add(wifi_score.wrapping_mul(5)))
        / 100;
    let grade = score_to_grade(total);

    FpsScore {
        latency: latency_score,
        jitter: jitter_score,
        packet_loss: loss_score,
        bufferbloat: bufferbloat_score,
        wifi: wifi_score,
        total,
        grade,
    }
}

/// FPS buyuk resim sentezi — oyun-turu kopru insight dahil.
fn synthesize_fps(fs: &FpsScore) -> Synthesis {
    let total = fs.total;
    let grade = score_to_grade(total);
    let mut insights = Vec::new();

    let headline_key = if total >= 90 {
        "synth_fps_competitive"
    } else if total >= 75 {
        "synth_fps_ranked"
    } else if total >= 60 {
        "synth_fps_hitreg"
    } else {
        "synth_fps_casual"
    }.to_string();

    // Pozitif: guclu metrikler
    if fs.latency >= 85 { insights.push(ins("positive", "synth_g_low_ping")); }
    if fs.jitter >= 85 { insights.push(ins("positive", "synth_g_stable")); }
    if fs.bufferbloat >= 85 { insights.push(ins("positive", "synth_g_no_bloat")); }
    if fs.packet_loss >= 90 { insights.push(ins("positive", "synth_g_no_loss")); }
    if fs.wifi >= 85 { insights.push(ins("positive", "synth_g_low_ping")); } // Ethernet/iyi WiFi

    // En zayif halka tespiti
    let weakest = [
        ("latency", fs.latency),
        ("jitter", fs.jitter),
        ("packet_loss", fs.packet_loss),
        ("bufferbloat", fs.bufferbloat),
        ("wifi", fs.wifi),
    ].iter().min_by_key(|(_, v)| *v).map(|(k, _)| *k).unwrap_or("latency");

    // Negatif/notr: en zayif halka
    if total < 90 {
        let key = match weakest {
            "latency" => "synth_g_weak_latency",
            "jitter" => "synth_g_weak_jitter",
            "packet_loss" => "synth_g_weak_loss",
            _ => "synth_g_weak_bloat",
        };
        let kind = if total < 60 { "negative" } else { "neutral" };
        insights.push(ins(kind, key));
    }

    // Oyun-turu kopru insight (KRITIK): zayif FPS ama MOBA/strateji/co-op sorunsuz
    let reflex_weak = matches!(weakest, "latency" | "jitter" | "packet_loss");
    if total < 75 && reflex_weak {
        insights.push(ins("neutral", "synth_fps_bridge_reflex"));
    } else {
        insights.push(ins("positive", "synth_fps_bridge_allgood"));
    }

    Synthesis { score: total, grade, headline_key, insights }
}

/// FPS Tanilama komutu — sadece FPS-ilgili moduller (latency/jitter/loss/bufferbloat/wifi).
#[tauri::command]
pub async fn run_fps_diagnosis(input: DiagnosticInput) -> Result<FpsDiagnosis, String> {
    let fps_score = compute_fps_score(&input);
    let fps_synthesis = synthesize_fps(&fps_score);

    // Sadece FPS-ilgili moduller
    let fps_modules: Vec<&str> = vec!["latency", "jitter", "loss", "bufferbloat", "wifi"];
    let modules = build_module_verdicts(&input)
        .into_iter()
        .filter(|m| fps_modules.contains(&m.module.as_str()))
        .collect();

    Ok(FpsDiagnosis { fps_score, fps_synthesis, modules })
}

fn local_analyze(
    input: &DiagnosticInput,
    score: &GamingScore,
) -> (String, Vec<String>, Vec<String>, String) {
    let mut issues = Vec::new();
    let mut suggestions = Vec::new();

    // Rule engine — deterministik kurallar
    let best_ping = extract_best_ping(&input.ping_results);
    let avg_jitter = extract_avg_jitter(&input.ping_results);
    let avg_loss = extract_avg_loss(&input.ping_results);
    let bloat_grade = input.bufferbloat
        .get("grade")
        .and_then(|v| v.as_str())
        .unwrap_or("F");
    let wifi_connected = input.wifi
        .get("connected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let wifi_signal = input.wifi
        .get("signal_percent")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if best_ping > 80.0 {
        issues.push(format!("Sunucu gecikmesi yüksek ({:.0}ms)", best_ping));
        suggestions.push("Daha yakın sunucu bölgesi seçin veya ISS'inizi değiştirmeyi düşünün.".to_string());
    }

    if avg_jitter > 10.0 {
        issues.push(format!("Jitter yüksek ({:.1}ms) — oyunda lag spike yaşarsınız", avg_jitter));
        if wifi_connected {
            suggestions.push("Ethernet kablosuna geçin, Wi-Fi jitter'ı artırır.".to_string());
        } else {
            suggestions.push("QoS ayarlarını kontrol edin, başka cihazlar bant genişliğini tüketiyor olabilir.".to_string());
        }
    }

    if avg_loss > 1.0 {
        issues.push(format!("Paket kaybı var (%{:.1}) — oyunda disconnect/teleport yaşarsınız", avg_loss));
        suggestions.push("Router'ı yeniden başlatın. Sorun devam ederse ISS'inizi arayın.".to_string());
    }

    if matches!(bloat_grade, "C" | "D" | "F") {
        let delta = input.bufferbloat
            .get("latency_delta_ms")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        issues.push(format!("Bufferbloat sorunu ({}ms gecikme artışı) — indirirken/yüklerken oyun lag yapar", delta as u32));
        suggestions.push("Router'da QoS veya SQM (Smart Queue Management) aktif edin.".to_string());
        suggestions.push("Oyun oynarken aynı ağda video izleme/torrent'ten kaçının.".to_string());
    }

    if wifi_connected && wifi_signal < 50 {
        issues.push(format!("Wi-Fi sinyal zayıf (%{})", wifi_signal));
        suggestions.push("Router'a daha yakın konumlanın veya Wi-Fi repeater kullanın.".to_string());
    }

    let summary = if issues.is_empty() {
        format!(
            "Ağ bağlantınız oyun için uygun görünüyor. Gaming skoru: {}/100 ({})",
            score.total, score.grade
        )
    } else {
        format!(
            "Gaming skoru: {}/100 ({}). {} sorun tespit edildi.",
            score.total,
            score.grade,
            issues.len()
        )
    };

    (summary, issues, suggestions, "local".to_string())
}

fn extract_best_ping(ping_results: &serde_json::Value) -> f64 {
    let arr = match ping_results.as_array() {
        Some(a) => a,
        None => return 999.0,
    };

    // Alan adı "name" ("game" değil). CS2/Valorant yoksa tüm games kategorisini dene,
    // o da yoksa reachable tüm sunucuların en iyisini al.
    let reachable: Vec<&serde_json::Value> = arr
        .iter()
        .filter(|r| r.get("reachable").and_then(|v| v.as_bool()).unwrap_or(false))
        .collect();

    if reachable.is_empty() {
        return 999.0;
    }

    // Önce CS2 veya Valorant sunucularını dene
    let gaming: Vec<f64> = reachable
        .iter()
        .filter(|r| matches!(
            r.get("name").and_then(|v| v.as_str()),
            Some("CS2") | Some("Valorant")
        ))
        .filter_map(|r| r.get("avg_ms").and_then(|v| v.as_f64()))
        .collect();

    if !gaming.is_empty() {
        return gaming.iter().cloned().fold(f64::MAX, f64::min).min(999.0);
    }

    // CS2/Valorant yoksa "games" kategorisindeki en iyi ping
    let games_cat: Vec<f64> = reachable
        .iter()
        .filter(|r| r.get("category").and_then(|v| v.as_str()) == Some("games"))
        .filter_map(|r| r.get("avg_ms").and_then(|v| v.as_f64()))
        .collect();

    if !games_cat.is_empty() {
        return games_cat.iter().cloned().fold(f64::MAX, f64::min).min(999.0);
    }

    // Son çare: tüm reachable sunucuların en iyisi
    reachable
        .iter()
        .filter_map(|r| r.get("avg_ms").and_then(|v| v.as_f64()))
        .fold(f64::MAX, f64::min)
        .min(999.0)
}

/// En iyi pingli reachable sunucunun jitter'ı.
/// Mantık: oyuncu en yakın sunucuya bağlanır — uzak Madrid sunucusunun
/// jitter'ı senin Frankfurt'taki oyun deneyimini etkilemez.
fn extract_avg_jitter(ping_results: &serde_json::Value) -> f64 {
    let arr = match ping_results.as_array() {
        Some(a) => a,
        None => return 0.0,
    };

    // Reachable sunucuları (avg_ms, jitter_ms) olarak topla
    let reachable: Vec<(f64, f64)> = arr
        .iter()
        .filter(|r| r.get("reachable").and_then(|v| v.as_bool()).unwrap_or(false))
        .filter_map(|r| {
            let avg = r.get("avg_ms").and_then(|v| v.as_f64())?;
            let jitter = r.get("jitter_ms").and_then(|v| v.as_f64())?;
            Some((avg, jitter))
        })
        .collect();

    if reachable.is_empty() {
        return 0.0;
    }

    // En düşük avg_ms'ye sahip sunucunun jitter'ını döndür
    reachable
        .iter()
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, j)| *j)
        .unwrap_or(0.0)
}

/// En düşük paket kaybı (reachable sunucular arasında).
/// Mantık: Hatın sağlığını en yakın/sağlıklı rota belirler. Uzak veya sorunlu
/// bir sunucunun yüksek loss'u senin baglantını kotu yapmaz — oyuncu en iyi
/// rotaya bağlanır. ICMP'yi bloklayan host'lar zaten reachable=false olduğu için
/// elenir.
fn extract_avg_loss(ping_results: &serde_json::Value) -> f64 {
    let arr = match ping_results.as_array() {
        Some(a) => a,
        None => return 0.0,
    };

    let losses: Vec<f64> = arr
        .iter()
        .filter(|r| r.get("reachable").and_then(|v| v.as_bool()).unwrap_or(false))
        .filter_map(|r| r.get("packet_loss").and_then(|v| v.as_f64()))
        .collect();

    if losses.is_empty() {
        return 0.0;
    }
    losses.iter().cloned().fold(f64::MAX, f64::min)
}

/// Skor (0-100) -> harf notu. 6 kademe: A+ A B C D F (akademik standart, E/S yok).
fn score_to_grade(score: u32) -> String {
    match score {
        95..=100 => "A+".to_string(),
        85..=94  => "A".to_string(),
        75..=84  => "B".to_string(),
        65..=74  => "C".to_string(),
        50..=64  => "D".to_string(),
        _        => "F".to_string(),
    }
}
