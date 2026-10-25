import { createContext, useContext } from "react";

/**
 * Paylasilan SENKRON calistirma kilidi (UI-01).
 *
 * Bant/gecikme olcen testler ayni anda tek bir test calistirir (biri digerinin olcumunu bozar).
 * KILITLI (acquire/release): hiz, ping listeleri (services/games), bufferbloat, throttle, ISP mudahale,
 * IPv6, Quick/Full tarama (App); VoIP, DNS benchmark, Twitch, LAN hiz istemcisi, traceroute (sekmeler).
 * Baslat butonlari TopBar/TestIntro/sekme icinde pasif olur + 'baska test calisiyor' ipucu gosterilir.
 *
 * Kilit App'teki bir ref'te tutulur -> acquire() cift tiklamada bile senkron olarak
 * dogru sonuc verir (state guncellemesini beklemez). `activeTest` yalnizca UI'nin
 * (buton disable + ipucu) yeniden cizilmesi icin state'tir.
 *
 * Kilide TABI OLMAYAN (pasif/hafif/surekli) araclar — bilincli istisna:
 *  - Network Stability izleyici ve Ping araci (surekli, 1 paket/sn mertebesinde hafif probe; kilitlerse
 *    saatlerce baska test baslatilamazdi; kullanici yuk altinda gecikmeyi izlemek icin ikisini birlikte kullanir);
 *  - Wi-Fi tarama (yerel radyo taramasi), LAN tarayici (yerel sweep);
 *  - CGNAT / MTU / DNS leak (kisa, dusuk bant hafif sorgular);
 *  - LAN hiz dinleyicisi (dinleme modu: bekler, trafik uretmez; yalnizca istemci testi kilitlenir).
 */
export interface TestGuard {
  /** Calisan testin kimligi; yoksa null. */
  activeTest: string | null;
  /** Kilidi almayi dener; baska bir test calisiyorsa false doner (kilit alinmaz). */
  acquire: (id: string) => boolean;
  /** Kilidi yalnizca sahibi (id) birakabilir. */
  release: (id: string) => void;
}

export const TestGuardContext = createContext<TestGuard>({
  activeTest: null,
  acquire: () => true,
  release: () => {},
});

export function useTestGuard(): TestGuard {
  return useContext(TestGuardContext);
}
