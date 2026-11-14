# aina notch (Rust + Tauri 2)

Aplikasi desktop aina untuk Windows: notch AI di tepi atas layar, giliran suara, "Kerjakan untukku", agent latar belakang dan aplikasi kerja. Server aina cloud ada di [ragam-karya/aina-backend](https://github.com/ragam-karya/aina-backend), landing page di [ragam-karya/aina-web](https://github.com/ragam-karya/aina-web).

Halaman UI (`ui/src/ui/*`: overlay, notch, gambar, Rive, Lottie, Settings, Agents, Ask, drop) berasal dari versi Electron dan dipakai apa adanya. Jembatannya `window.aina` (`crates/aina-app/src/shim.js`).

## Struktur

```
aina-notch/
├─ crates/
│  ├─ aina-apps/    aplikasi kerja Composio (REST tool-router untuk ak_, MCP untuk ck_), upload Drive, media (WinRT) (port workApps.js, media.js)
│  ├─ aina-net/     ke mana panggilan pergi: langsung ke penyedia (key sendiri) atau lewat aina Cloud (token akun)
│  ├─ aina-app/     biner Tauri: jendela, tray, IPC window.aina, demo
│  ├─ aina-agent/   "Kerjakan untukku" + aina latar belakang: loop Brain, computer toolset, file tools + guard perintah, apps/URL, bridge ekstensi, agent (store, mesin sendiri, Claude Code, Codex), mode fokus (port brain/*, fileOps.js, agentTools.js, agents.js, agentLocal.js, focus.js)
│  ├─ aina-core/    logika tanpa OS: timer + pengingat, arti sebuah drop, baris status agent (port timers.js, notch.js)
│  ├─ aina-input/   hook keyboard+mouse, hotkey tahan-bicara, SendInput, parser key, gesture (port hotkeys.js, brain/input.js, brain/keys.js, gesture.js)
│  ├─ aina-llm/     protokol balasan: tag visual + StepSplitter (port tags.js, steps.js, beserta tesnya)
│  ├─ aina-screen/  tangkap layar per monitor + JPEG + crop tanda, matematika koordinat (port capture.js, region.js, brain/coords.js)
│  ├─ aina-uia/     UI Automation lewat COM di dalam proses (port uia.js + BadiUia.cs)
│  └─ aina-store/   config.json (%APPDATA%\aina) + key di Windows Credential Manager (service `aina`)
├─ ui/             halaman UI (src/ui), string i18n (src/i18n.js), aset: logo, font Urbanist, Rive, Lottie
├─ extension/      ekstensi browser (MV3) untuk "Kerjakan untukku" di browser
├─ native/wake/    pendengar "Hey aina" (AinaWake.cs, System.Speech)
├─ aina-mascot/    mascot Rive (proyek Rive CLI → dist/BADI_Mascot.riv)
├─ store/          listing + screenshot Microsoft Store
├─ scripts/
│  ├─ sync-ui.mjs   salin ui/ ke ui-dist/ dengan tata letak yang sama (path relatif tetap jalan)
│  ├─ build-wake.mjs  kompilasi AinaWake.exe dengan csc.exe bawaan Windows
│  └─ brand-icon.ps1  ikon dan logo dari icons/brand
├─ docs/           DESIGN-SYSTEM.md, PRD, BRD, QA-paritas.md, hardening.md
└─ ui-dist/         hasil sync-ui (tidak di-commit)
```

## Menjalankan

Butuh: Rust (MSVC), **Visual Studio Build Tools dengan "Desktop development with C++"**, Node 20+, lalu `npm install` (runtime Rive/Lottie dan Tauri CLI). `npm run dev`/`build` otomatis mengompilasi `AinaWake.exe` dan menjalankan sync-ui.

> Di mesin pengembang, Build Tools terpasang di `D:\VS\BuildTools` (disk C hampir penuh). Jalankan `cargo` dari **PowerShell**, bukan Git Bash: `link.exe` milik Git Bash menimpa linker MSVC dan build gagal dengan "extra operand".

```powershell
npm install
npm test                      # cargo test --workspace (tes yang di-port dari v1)
npm run dev                   # sync-ui + tauri dev
$env:AINA_DEMO = "1"; npm run dev   # giliran skrip: notch + POINT + tulisan tangan + lingkaran
$env:AINA_DEMO = "notch"; $env:AINA_DEMO_OUT = "D:\shots"; npm run dev   # semua mode notch, PNG per langkah (sama dengan v1)
$env:AINA_DEMO = "talk"; $env:AINA_DEMO_OUT = "D:\shots"; npm run dev    # tahan-bicara sintetis: lingkaran → tanda → capture → UIA
cargo test -p aina-uia --test apps -- --ignored --test-threads=1     # UIA ke Calculator + jendela WinForms sungguhan
$env:AINA_PROFILE = "D:\aina-test"  # data di folder lain, config asli tidak tersentuh
```

### Build rilis
```powershell
$env:AINA_CLOUD_URL = "https://be.aina.ai.id"                  # alamat Cloud ikut di-build (default-nya juga ini)
$env:AINA_WEB_URL = "https://aina.ai.id/api"                            # opsional (ini default-nya): halaman Ketentuan dan Privasi yang dibuka di browser, lewat proxy /api website
$env:AINA_FEEDBACK_URL = "https://forms.gle/…"                            # opsional: tray → Kirim masukan
npm run build    # → target\release\bundle\nsis\aina_<versi>_x64-setup.exe
```
Server aina cloud (Worker Cloudflare + `badi-cloud-core`) dan landing page ada di repo terpisah: [ragam-karya/aina-backend](https://github.com/ragam-karya/aina-backend) dan [ragam-karya/aina-web](https://github.com/ragam-karya/aina-web).

## Status (Fase 0–9)

| Bagian | Status |
|---|---|
| Parser tag + StepSplitter + gesture + timer/pengingat + drop/agent line + key + koordinat + region (Rust) + tes v1 | ✅ 41 tes lulus + 2 tes integrasi UIA, clippy bersih |
| Hotkey tahan-bicara lewat hook keyboard (Ctrl palsu dari AltGr diabaikan; input aina sendiri ditandai dan dilewati) | ✅ Sempat mati total sampai 19 Okt 2026 (antrean tombol diambil pelacak kursor). Sudah diperbaiki dan diuji dengan tombol simulasi: tahan → mendengar → menyalin suara. Combo baru di Settings langsung berlaku. |
| SendInput: klik, drag, scroll, kombinasi tombol, ketik Unicode | ✅ kode (dipakai Brain di Fase 6) |
| Tangkap layar per monitor (GDI), resize 1024/1280, JPEG, crop area yang ditandai | ✅ siap ±1 detik setelah tombol dilepas di build debug |
| UI Automation di dalam proses (pengganti BadiUia.exe): list_windows, get_window_state, click, set_value, focus_window, element_at | ✅ Calculator 1+2=3 lewat ref, field password ditolak, `element_at` 17 ms |
| Tinta saat bicara: lingkaran / garis bawah / tunjuk → tanda + capture awal + nama elemen di bawahnya | ✅ `AINA_DEMO=talk` |
| Suara (Fase 4): mic hangat 20 detik + level ke notch, STT OpenAI, TTS ElevenLabs → OpenAI → suara Windows dengan hedging + cache, pemutar dengan event mulai, Esc/tahan memotong suara | ✅ tes API nyata: TTS 0,8 s, STT 1,9 s, transkrip tepat (`cargo test -p aina-voice --test apis -- --ignored`) |
| "Hey aina" (AinaWake.exe + tanya hands-free dengan deteksi jeda), mesin realtime (WebRTC di jendela worker v1, logika conversation.js di Rust), dikte Ctrl+Alt (disimpan sebagai catatan .md di Documents\aina\Dikte, dirapikan + diringkas Haiku, tampil di notch, scroll kalau panjang), chime, filler 1,5 dtk + prefetch, sapaan saat start, timer 6 dtk + petunjuk Esc 20 dtk | ✅ kode + tes (vad, chime, persona). **Belum diuji dengan mic sungguhan.** Realtime lewat aina Cloud: Cloud mencetak secret 60 dtk dan menagih tiap balasan dari usage yang dilaporkan app. |
| Companion (Fase 5): Claude lewat HTTP+SSE dengan cache prompt + prewarm + fallback, prompt v1 verbatim, StepSplitter → suara per kalimat → visual saat suara mulai → tunggu gambar selesai, riwayat 10 menit, Guided klik/hover, TIMER/REMIND/DROP, meter biaya | ✅ `AINA_DEMO=ask`: token pertama 830 ms, Rp 138/pertanyaan (cache 4.201 token), kursor menunjuk tombol × dengan tepat |
| "Kerjakan untukku" (Fase 6): `[DO]` → izin sekali (`computerControl`) → loop Brain append-only (beta fallback + thinking updates + context management, clear_tool_uses 60k/keep 4/≥20k), computer toolset di monitor yang dilihat, screenshot otomatis 450 ms setelah aksi yang mengubah layar, HALT setelah gagal, web_search/web_fetch, open_app/open_url, UIA, file tools + edit tools (guard, Recycle Bin, tidak menimpa, CRLF), save_file ke Documents\aina, pertanyaan di notch (Boleh/Boleh semua/Jangan, suara, Enter/Esc), meter biaya | ✅ 17 tes baru (loop dengan model palsu, guard, file ops nyata, safeUrl/matchApps/targetPath, bridge dengan ekstensi palsu). `AINA_DEMO=brain`: Calculator 12×3 lewat UIA, 4 giliran, 17 detik, $0,085 |
| Bridge ekstensi browser: WebSocket 127.0.0.1:47821, Origin ekstensi + kode 6 digit (constant-time), kunci 10 salah/menit, koneksi terbaru menang, status ke Settings | ✅ diuji dengan klien palsu. Belum diuji dengan ekstensi Chrome sungguhan. |
| Dialog izin kontrol komputer | MessageBox Windows (OK/Batal), bukan tombol "Izinkan / Jangan sekarang" v1 |
| aina latar belakang (Fase 7): workspace `agents/<slug>/` sama dengan v1 (agent lama terbuka), jendela Agents (daftar, chat, log, stop, folder, routine), `[DELEGATE]` ke General atau agent proyek, NEXT_ACTIONS/ARTIFACTS/ROUTINE, biaya per balasan, notice + notifikasi Windows saat selesai, status di notch | ✅ 14 tes baru (store, parseBlocks, guard PowerShell v1 lengkap, approvalFor, Remove-ToRecycleBin sungguhan, mesin lokal, fokus) |
| Mesin: Claude Code milik pengguna lewat CLI (`-p --output-format stream-json`, config dir sementara), guard lewat hook PreToolUse (`aina.exe --agent-hook`) dan tool aina lewat MCP (`aina.exe --agent-mcp`) yang tersambung ke app dengan token per tugas; mesin sendiri (text editor, PowerShell, cari file native, web) sebagai cadangan; Codex opsional | ✅ `AINA_DEMO=agent`: tulis file + `list_folder` via MCP, $0,041. Hook terbukti dipanggil Claude Code dan menolak saat aina tak terjangkau (fail-closed) |
| Pertanyaan agent di notch (antre sampai aina diam, "boleh semua"), mode fokus (`[FOCUS]`, folder di-drop, cari BFS, akhiri dari notch), routine yang lanjut setelah PC tidur | ✅ kode + tes. Routine belum diuji melewati sleep sungguhan. |
| Aplikasi kerja (Fase 8): Composio tanpa SDK Node — key `ak_` lewat REST tool-router (session, meta tool, status, link, auth config white-label, upload Drive via storage S3), key `ck_` lewat MCP streamable HTTP; saklar per aplikasi, izin bertanya lewat suara (`appQuestion`), `connect_app`, `upload_to_drive`, Settings → Aplikasi kerja, Brain dan agent (bukan agent proyek) | ✅ 4 tes baru + tes live read-only: key v1 terbaca, 3 meta tool, Gmail + Calendar tersambung. Agent "cek 3 email terbaru" lewat Composio: $0,088. Belum diuji nyata: kirim email (butuh izin), upload Drive, white-label, key `ck_` |
| Key v1 (`enc:` safeStorage) dibaca langsung (DPAPI + AES-GCM dari `chromium\Local State`), tidak diubah | ✅ Anthropic, OpenAI, ElevenLabs, Composio terbaca dari config v1 |
| Media di notch lewat WinRT langsung (pengganti BadiMedia.exe), `[MEDIA]`, tombol media notch, fallback tombol media; pintasan notch; pertanyaan diketik (Ask) | ✅ kode; `now()` diuji (tidak ada yang diputar saat uji) |
| Drop folder fokus: zona + animasi baru muncul saat folder diseret ke notch (sebelumnya aina cukup bilang bisa di-drag ke notch); notice "📁 Fokus · folder" + pill notch yang terus menunjukkan folder fokus | ✅ `AINA_DEMO=notch` PNG: diam → zona muncul saat diseret → notice → chip fokus |
| Notch pintar di core: panel, notice, pertanyaan Boleh/Boleh semua/Jangan, zona drop (jendela drop di atas notch), timer + pengingat, status peek, tombol notch | ✅ `AINA_DEMO=notch` + `AINA_DEMO_OUT` dibandingkan PNG per langkah dengan v1: tampilan sama |
| Hover notch lewat hook mouse low-level (bukan polling) | ✅ proses Rust 0 ms CPU saat diam; renderer WebView2 0 ms |
| Overlay dibuat ulang saat monitor dicolok/dicabut/diubah skalanya; posisi hanya dikirim ke overlay monitornya | ✅ kode. Belum diuji fisik di 2 monitor. |
| Config v1 dibaca apa adanya, key di Credential Manager, key `enc:` v1 dibiarkan (rollback aman) | ✅ |
| Overlay per monitor (transparan, click-through, content-protected) + notch, Rive, tulisan tangan, bentuk | ✅ diverifikasi lewat screenshot `AINA_DEMO` di 1 monitor. **Belum diuji di 2 monitor dengan DPI berbeda** (gerbang spike Fase 0). |
| Maskot aina di notch (`aina-mascot/`, Rive `Compact` lewat view model `BADI`): satu instance dan satu canvas per overlay (baris turn, pil mini, samping jam di peek), 11 ekspresi dari fase notch, hover/klik/goyang dari halaman, gambar hanya saat tampil | ✅ screenshot `AINA_DEMO=notch`/`mini`. Benchmark `scripts/perf-mascot.ps1` (A mata lama, B maskot naif, C maskot final, idle 30 menit + 4 skenario): idle 0,02% CPU dan 0% GPU, animasi 4,95% CPU vs 5,11% di A, satu instance Rive (init 120 ms), heap kembali ke ~5,7 MB, hide/show kini berhenti menggambar (CPU 5,56% → 3,25%). Hasil di `perf/` (tidak di-commit) |
| Settings / Agents / Ask memakai halaman v1 | ✅ Semua kanal v1 sudah ditangani (termasuk tes key Anthropic dan folder ekstensi). Ask menutup setelah dikirim dan saat kehilangan fokus. Jalan saat Windows mulai lewat HKCU Run. |
| Tray dengan menu v1, single-instance | ✅ Tooltip menunjukkan status, folder fokus, dan tombol bicara. "Hentikan jawaban" sama dengan Esc. |
| Input/hotkey, capture, UIA, audio, LLM, Brain, agent, apps | Fase 3–8 |
| aina Cloud: login Google di browser, kembali ke app lewat `aina://` (state milik app, kode sekali pakai), kredit (Free 10 sekali per PC, langganan Basic/Pro 30 hari, Top-Up Small/Medium/Large, Enterprise; tanpa batas sesi atau mingguan, peringatan saat kredit hampir habis, tugas berjalan boleh selesai), Midtrans Snap (sandbox), proxy Claude/OpenAI/ElevenLabs/Composio dengan biaya dicatat dari usage, hapus akun | ✅ aturan: 10 tes `badi-cloud-core`; app: Settings → Akun, chip sisa kredit di notch, model dan suara mengikuti paket. ⏳ Worker (`cloud/`) belum dikompilasi atau dideploy: internet sedang putus |
| Hardening (Fase 9): CSP Tauri ketat + `freezePrototype`, review keamanan, perbandingan performa | ✅ v2 vs v1 saat diam: RAM −28% (400 vs 553 MB), CPU 0,04% vs 0,33%, exe 9,2 MB vs installer 115,9 MB. Lihat `docs/hardening.md` |
