# Fase 9: hardening

## Performa (diukur 19 Okt 2026, laptop pengembang, profil baru, suara mati, 25 detik setelah start, rata-rata 20 detik)

| | v2 (Tauri, release) | v1 (Electron) | Selisih |
|---|---|---|---|
| RAM (working set, semua proses) | 400 MB · 7 proses | 553 MB · 6 proses | −28% |
| RAM privat | 204 MB | 284 MB | −28% |
| CPU saat diam | 0,04% | 0,33% | ±8× lebih hemat |
| Ukuran | `badi.exe` 9,2 MB (UI tertanam) | installer 115,9 MB | ±12× lebih kecil |

Catatan:
- RAM v2 didominasi proses WebView2: overlay per monitor, notch, dan proses GPU/utility bersama. Peluang berikutnya: membuat jendela Settings, Agents, dan Ask hanya saat dibuka (sudah begitu), dan mencoba `--disable-features` WebView2 untuk overlay.
- Installer NSIS v2 belum dibangun karena bundler Tauri mengunduh NSIS dan internet sedang mati. Perkiraannya ±4–6 MB, ditambah bootstrapper WebView2 untuk Windows 10 lama.
- Latensi giliran tanya sudah diukur di Fase 5 (token pertama ±830 ms). Waktu start belum diukur otomatis.

Cara mengulang: `powershell -File scripts\perf.ps1`, setelah `cargo build -p badi-app --release`. Profil ujinya `D:\badi-perf-v1` dan `D:\badi-perf-v2`.

## Review keamanan

| Area | Keadaan | Bukti |
|---|---|---|
| Halaman (WebView) | CSP Tauri ketat (`default-src 'self'`, script tanpa inline, `wasm-unsafe-eval` hanya untuk Rive, `object-src 'none'`, `frame-ancestors 'none'`). Halaman v1 juga punya CSP meta sendiri. `freezePrototype` aktif. | Demo notch jalan normal dengan CSP baru, tanpa pelanggaran CSP di log. |
| Jembatan `window.badi` | Allowlist kanal sama dengan v1, diperiksa di `shim.js` **dan** di `ipc.rs` (`INVOKE`). Kanal lain ditolak. | Kode `ipc.rs` |
| Key | Disimpan di Windows Credential Manager, tidak pernah di config.json. Key v1 (`enc:`) hanya dibaca dan tidak diubah, jadi rollback aman. Token Badi Cloud juga di Credential Manager. | `badi-store` |
| Brain / agent | Guard v1 di-port penuh, dengan semua kasus tes v1. Hapus selalu ke Recycle Bin. Perubahan di luar folder agent ditanyakan dulu. | 31 tes badi-agent |
| Claude Code | Setiap tool lewat hook PreToolUse (`badi.exe --agent-hook`) ke link lokal bertoken acak per tugas. Kalau Badi tak terjangkau, tool ditolak (fail-closed). Config dir sementara dihapus setelah tugas. | Uji live Fase 7 dan 8: tulis ke `C:\Windows` ditolak hook |
| Ekstensi browser | Hanya `127.0.0.1`, Origin ekstensi + kode 6 digit (dibandingkan constant-time), kunci setelah 10 salah per menit. | Tes bridge |
| Skema `badi://` | Didaftarkan di HKCU (tanpa admin). Link login hanya diterima untuk `state` yang dibuat app itu sendiri dalam 15 menit. Kodenya sekali pakai, 5 menit, dan disimpan sebagai hash. | `cloud.rs`, `badi-cloud-core` |
| Badi Cloud | Key penyedia hanya di secret Worker. Token perangkat dan kode login disimpan sebagai hash. Notifikasi Midtrans dicek tanda tangan SHA-512 + jumlah, dan lunas hanya sekali. Composio dipaksa ke `user_id` akun, 17 aplikasi, dan konfigurasi login server; session dan akun tertaut hanya milik sendiri. Model dan suara dibatasi paket. | 10 tes `badi-cloud-core` |

## Belum (sebelum beta publik)
- **Code signing** installer dan exe (BO-4). Tanpa ini SmartScreen memperingatkan pengguna.
- **Updater Tauri**: butuh kunci tanda tangan update dan endpoint (bisa dari Worker yang sama).
- **Worker dikompilasi dan dideploy:** perlu internet, `wasm32-unknown-unknown`, wrangler, akun Cloudflare, D1, OAuth client Google, dan Midtrans sandbox.
- **Uji fisik:** dua monitor dengan DPI berbeda, mic sungguhan + hotkey oleh pengguna, dan routine melewati sleep.
- **Pentest** Worker oleh orang lain, dan review hukum halaman privasi/ketentuan (UU PDP).
- **Beta tertutup** 10–20 orang dengan `docs/QA-paritas.md`.
