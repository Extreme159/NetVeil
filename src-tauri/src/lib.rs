use tauri::Manager;

/// Debug log macro — sadece debug build'de çalışır, release'de no-op olur.
/// Portable .exe çıktısı sessiz olsun, console penceresi açılmasın.
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => { println!($($arg)*) };
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {};
}

mod nic;
mod wifi;

/// Benzersiz u16 ICMP kimliği üretir (PingIdentifier vb. için).
///
/// Önceki sürüm `subsec_nanos() % 65535` kullanıyordu; eşzamanlı çağrılarda
/// (aynı nanosaniye penceresinde) aynı identifier üretilebiliyordu — eşzamanlı
/// ICMP oturumlarında cevap karışmasına yol açar (rapor maddesi).
///
/// Bu sürüm process'e özgü rastgele bir taban (açılışta bir kez) ile monotonik
/// artan atomic sayacı birleştirir: tek process içinde ardışık çağrılar asla
/// çakışmaz, farklı process'ler farklı tabanla başlar.
pub fn rand_id() -> u16 {
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU16 = AtomicU16::new(0);
    static BASE: OnceLock<u16> = OnceLock::new();

    let base = *BASE.get_or_init(|| {
        let pid = std::process::id() as u16;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        pid ^ (nanos as u16) ^ ((nanos >> 16) as u16)
    });

    base.wrapping_add(COUNTER.fetch_add(1, Ordering::Relaxed))
}
mod ping;
mod bufferbloat;
mod speedtest;
mod network_topology;
mod traceroute;
mod smart;
#[cfg(feature = "premium")]
mod throttle;
mod throttle_baseline;
#[cfg(feature = "premium")]
mod throttle_bittorrent;
#[cfg(feature = "premium")]
mod throttle_quic;
#[cfg(feature = "premium")]
mod throttle_wireguard;
#[cfg(feature = "premium")]
mod throttle_openvpn;
#[cfg(feature = "premium")]
mod isp_intervention;
mod ipv6;
mod dns_benchmark;
mod wifi_scan;
#[cfg(feature = "premium")]
mod network_stability;
mod db;
mod ping_test;
#[cfg(feature = "premium")]
mod twitch_test;
#[cfg(feature = "premium")]
mod twitch_rtmp;
#[cfg(feature = "premium")]
mod cgnat;
mod mtu;
#[cfg(feature = "premium")]
mod stun;
#[cfg(feature = "premium")]
mod dns_leak;
#[cfg(feature = "premium")]
mod voip_test;
#[cfg(feature = "premium")]
mod lan_scanner;
#[cfg(feature = "premium")]
mod mdns;
#[cfg(feature = "premium")]
mod lan_speed;
mod license;


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // rustls için ring provider'ı global olarak kur (DoT/DoH TLS gerektirir)
    // Birden fazla thread'den çağrılsa da install_default hata döndürür, panic etmez
    let _ = rustls::crypto::ring::default_provider().install_default();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            // Tray ikonu kur — ikon koddan verilir (gömülü pencere ikonu).
            // NOT: tauri.conf trayIcon bloğu KALDIRILDI; config-driven + kod-driven
            // ikisi birden tray oluşturup 2 ikon (biri boş) gösteriyordu.
            let _tray = tauri::tray::TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("NetVeil — Ağ Kararlılık Monitörü")
                .on_tray_icon_event(|tray, event| {
                    use tauri::tray::TrayIconEvent;
                    if let TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. } = event {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;
            // Açılışta tüm node listelerini sunucudan paralel çek (arka planda, UI'yı bloklamaz)
            tauri::async_runtime::spawn(async {
                tokio::join!(
                    ping::init_ping_targets(),
                    ping::init_gfn_targets(),
                    dns_benchmark::init_dns_providers(),
                );

            });
            Ok(())
        });

    #[cfg(feature = "premium")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        nic::get_nic_info,
        wifi::get_wifi_info,
        ping::ping_game_servers,
        ping::get_ping_targets_list,
        bufferbloat::run_bufferbloat_test,
        speedtest::run_speed_test,
        smart::run_smart_diagnosis,
        smart::run_module_verdict,
        smart::run_fps_diagnosis,
        network_topology::get_topology,
        network_topology::ping_gateway,
        network_topology::ping_host,
        network_topology::check_for_update,
        traceroute::run_traceroute,
        ping_test::run_ping_test,
        ping_test::stop_ping_test,
        ipv6::run_ipv6_test,
        dns_benchmark::run_dns_benchmark,
        wifi_scan::run_wifi_scan,
        mtu::run_mtu_test,
        db::save_test_run,
        db::list_test_runs,
        db::list_test_runs_by_module,
        license::get_license_status,
        license::activate_license,
        license::validate_license,
        license::deactivate_license,
        network_stability::start_stability_monitor,
        network_stability::stop_stability_monitor,
        network_stability::get_stability_state,
        throttle::run_throttle_test,
        isp_intervention::run_isp_intervention_test,
        twitch_test::run_twitch_test,
        cgnat::run_cgnat_test,
        dns_leak::run_dns_leak_test,
        voip_test::run_voip_quality_test,
        lan_scanner::run_lan_scan,
        lan_scanner::stop_lan_scan,
        lan_speed::start_lan_speed_server,
        lan_speed::stop_lan_speed_server,
        lan_speed::run_lan_speed_test,
        lan_speed::get_local_hostname,
        lan_speed::discover_lan_speed_peers,
    ]);

    #[cfg(not(feature = "premium"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        nic::get_nic_info,
        wifi::get_wifi_info,
        ping::ping_game_servers,
        ping::get_ping_targets_list,
        bufferbloat::run_bufferbloat_test,
        speedtest::run_speed_test,
        smart::run_smart_diagnosis,
        smart::run_module_verdict,
        smart::run_fps_diagnosis,
        network_topology::get_topology,
        network_topology::ping_gateway,
        network_topology::ping_host,
        network_topology::check_for_update,
        traceroute::run_traceroute,
        ping_test::run_ping_test,
        ping_test::stop_ping_test,
        ipv6::run_ipv6_test,
        dns_benchmark::run_dns_benchmark,
        wifi_scan::run_wifi_scan,
        mtu::run_mtu_test,
        db::save_test_run,
        db::list_test_runs,
        db::list_test_runs_by_module,
        license::get_license_status,
        license::activate_license,
        license::validate_license,
        license::deactivate_license,
    ]);

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
