# PRD: aina

| | |
|---|---|
| **Produk** | aina (dulu B.A.D.I. / "Badi"), asisten AI untuk Windows yang paham layar |
| **Versi produk** | 1.0.0 (Rust + Tauri 2, folder `badi-v2`, akan jadi `aina-v1.0.0`) |
| **Dokumen** | Product Requirements Document |
| **Status** | Draft untuk review |
| **Pemilik** | Faisal (Ragam Karya) |
| **Tanggal** | 3 November 2026 |
| **Terkait** | [BRD.md](BRD.md), [../../BRANDING.md](../../BRANDING.md), [BETA.md](BETA.md), [QA-paritas.md](QA-paritas.md), [hardening.md](hardening.md), [../cloud/README.md](../cloud/README.md), [../store/listing.md](../store/listing.md) |

> Dokumen ini menggantikan `../../docs/PRD.md` (v1 Electron, 17 Okt 2026) untuk versi Tauri. v1 tidak dikembangkan lagi. Nama teknis lama (`badi-*`, `window.badi`, `BADI_*`, `badi://`, `%APPDATA%\BADI`) tetap dipakai di kode; teks yang dilihat pengguna memakai **aina**, huruf kecil semua.

---

## 1. Ringkasan

aina adalah app tray untuk Windows. Pengguna menahan `Ctrl+Alt` (atau mengetuknya, atau bilang "Hey aina"), lalu bicara. aina melihat layar yang relevan, menjawab dengan suara, dan menunjuk langsung di layar: kursor biru, sorotan, lingkaran, panah, dan tulisan tangan. aina juga bisa:
- memandu klik demi klik;
- mengoperasikan komputer atas izin pengguna ("Kerjakan untukku");
- merapikan dan mengedit file, selalu dengan izin dan lewat Recycle Bin;
- fokus di satu folder proyek;
- menjalankan pekerjaan panjang lewat **Sprite**, helper latar belakang;
- memakai aplikasi kerja (Gmail, Calendar, Drive, Docs, Sheets, Tasks);
- menjawab dari file milik pengguna (pengetahuan) dan menyimpan dikte sebagai catatan.

Di tepi atas layar ada **notch pintar** yang gratis tanpa akun: jam, cuaca, timer dan pomodoro, pengingat dan kalender, jadwal salat, musik, catatan, fokus, dan pintasan. Fitur AI berjalan lewat **aina Cloud** dengan kredit; app tidak menyimpan API key apa pun.

Prinsip produk ([BRANDING.md](../../BRANDING.md)):
- hadir tanpa menyela;
- tunjukkan, jangan cuma ceritakan;
- satu langkah dalam satu waktu;
- privasi sebagai default;
- pengguna yang memegang kendali.

### Positioning
> Untuk pekerja kantoran dan kreator di Indonesia yang sehari-hari memakai aplikasi rumit di Windows, **aina adalah asisten AI yang paham layar**: duduk di sebelah kursor, melihat isi layar, dan menunjuk tombol yang tepat lewat suara. Berbeda dari AI di kotak chat dan asisten buatan luar negeri, aina berbahasa Indonesia sejak awal dan selalu bertanya sebelum bertindak.

### Perubahan utama dari v1
| Area | v1 (Electron) | v1.0.0 (Tauri) |
|---|---|---|
| Nama | B.A.D.I. / Badi | aina (huruf kecil) |
| Key AI | BYOK, dienkripsi `safeStorage` | **Tidak ada key di PC.** Semua lewat aina Cloud dengan token akun di Windows Credential Manager |
| Monetisasi | Belum ada | Kredit: Free, Basic, Pro, Top-Up, Enterprise (Midtrans) |
| Agent latar belakang | "Badi latar belakang", agent bebas | **Sprite**: 6 jenis spesialis, maks. 8 bekerja bersamaan |
| Notch | Status, izin, drop, timer | + Home, cuaca, jadwal salat, Pusat Pengingat + Google Calendar, musik interaktif + lirik, Fokus dengan riwayat, catatan, pengetahuan, maskot Rive, sembunyi di layar penuh |
| Ukuran dan sumber daya | Installer ±116 MB, RAM 553 MB, CPU diam 0,33% | `badi.exe` 9,2 MB, RAM 400 MB, CPU diam 0,04% |
| Distribusi | Installer NSIS | Installer NSIS (unduh dari web) + Microsoft Store (MSIX, `RagamKarya.Aina`) |

## 2. Masalah

1. **Berpindah konteks.** Asisten AI hidup di jendela chat. Pengguna harus pindah jendela, menjelaskan isi layar dengan kata-kata, lalu menerjemahkan jawabannya kembali ke aplikasi.
2. **Jawaban teks sulit diikuti.** "Klik Format → Conditional formatting → …" lebih lambat daripada ditunjuk langsung.
3. **Pekerjaan kecil yang berulang.** Merapikan Unduhan, mengecek email, menjadwalkan meeting, merangkum dokumen. Bisa didelegasikan, asal aman.
4. **Takut AI bertindak sendiri.** Pengguna mau dibantu, tapi tidak mau file terhapus atau email terkirim tanpa sepengetahuannya.
5. **Hambatan setup.** Mengisi API key dari beberapa penyedia (v1) terlalu sulit bagi pengguna awam.

## 3. Tujuan dan non-tujuan

### Tujuan
- **G1.** Pertanyaan tentang layar dijawab dengan suara dan visual tanpa pindah jendela.
- **G2.** Tugas di komputer, file, dan aplikasi kerja bisa didelegasikan lewat suara; setiap perubahan tetap seizin pengguna.
- **G3.** Pemakaian pertama tanpa setup teknis: pasang, masuk dengan Google, langsung bertanya.
- **G4.** Biaya per tugas rendah, terukur, dan terlihat sebagai sisa kredit.
- **G5.** Tidak ada jejak data pribadi di disk selain yang diminta pengguna; app ringan saat diam.

### Non-tujuan (versi ini)
- macOS, Linux, ponsel, wearable, TV (visi, bagian 12).
- Berjalan tanpa internet. Model AI ada di cloud. (Notch tetap jalan offline untuk fitur lokal.)
- Bertindak tanpa izin di luar yang diizinkan Settings.
- BYOK (key sendiri) di build publik.
- Fitur enterprise: SSO, admin console, kebijakan organisasi.

## 4. Pengguna

| Persona | Kebutuhan | Fitur utama |
|---|---|---|
| **Pekerja kantor** (admin, marketing, ops) | Cepat menemukan menu, membereskan email, kalender, dokumen | Tanya layar, Guided, Aplikasi kerja, Pengingat, Dikte |
| **Developer** | Review diff, cari bug/celah keamanan, perbaikan cepat di satu proyek | Mode Fokus, Sprite Ciro, laporan `badi-reports` |
| **Pengguna awam** | Dipandu langkah demi langkah tanpa jargon | Guided, suara, Ngobrol (realtime) |
| **Power user** | Mendelegasikan tugas panjang dan berulang | Kerjakan untukku, Sprite, routine, file tools |
| **Pengguna notch saja** | Jam, timer, pomodoro, pengingat, salat, musik tanpa membayar | Notch pintar (gratis, tanpa akun) |

## 5. Ruang lingkup fitur

Prioritas: **P0** wajib di rilis, **P1** penting, **P2** bagus kalau ada. Status menunjukkan kondisi kode v1.0.0 per 3 Nov 2026 (sumber: [README](../README.md), [QA-paritas](QA-paritas.md)).

| ID | Fitur | Prioritas | Akses | Status |
|---|---|---|---|---|
| F1 | Tanya layar (tahan untuk bicara) | P0 | Kredit | Ada, teruji (`BADI_DEMO=ask`) |
| F2 | Menandai sambil bicara (lingkari, garis bawahi, tunjuk) | P1 | Kredit | Ada, teruji (`BADI_DEMO=talk`) |
| F3 | Mode Guided (klik demi klik) | P0 | Kredit | Ada |
| F4 | Dikte jadi catatan | P1 | Kredit | Ada; belum diuji dengan mic sungguhan oleh penguji |
| F5 | Ngobrol realtime dan "Hey aina" | P1 | Kredit | Ada (opsional, default `classic`); belum diuji dengan mic sungguhan |
| F6 | Kerjakan untukku (kontrol komputer dan browser) | P0 | Kredit | Ada, teruji (`BADI_DEMO=brain`); ekstensi belum diuji dengan Chrome sungguhan |
| F7 | File tools dan izin lewat suara | P0 | Kredit | Ada, 31 tes |
| F8 | Mode Fokus | P1 | Notch gratis; AI kredit | Ada |
| F9 | Sprite (helper latar belakang) dan routine | P1 | Kredit | Ada; routine belum diuji melewati sleep |
| F10 | Aplikasi kerja (Composio) | P1 | Berbayar | Ada; hanya aplikasi Google yang dibuka, lainnya "Segera" |
| F11 | Pengetahuan (file pengguna) dan catatan | P1 | Notch gratis; tanya kredit | Ada |
| F12 | Notch pintar | P0 | Gratis, tanpa akun | Ada, dibandingkan PNG per langkah dengan v1 |
| F13 | Akun aina Cloud dan kredit | P0 | — | Production sejak 20 Okt 2026 (Midtrans masih sandbox) |
| F14 | Settings, onboarding, i18n (id/en) | P0 | — | Ada |
| F15 | Instalasi, update, dan distribusi | P0 | — | NSIS + MSIX ada; code signing dan updater belum |

### F1. Tanya layar
- **FR-1.1** Menahan tombol bicara merekam suara dan mengambil screenshot lebih awal: 300 ms setelah ditahan dan setiap ada tanda. Hanya layar tempat kursor atau tanda berada, lebar 1024 px.
- **FR-1.2** Jawaban Claude di-stream dan dipotong per kalimat (`StepSplitter`). Visual tiap langkah tampil tepat saat suaranya mulai; langkah berikutnya menunggu suara dan gambar selesai.
- **FR-1.3** Protokol tag visual: `[POINT] [TARGET] [HOVER] [HIGHLIGHT] [SHAPE] [WRITE] [DRAW]`, plus `[DO] [DELEGATE] [FOCUS]` dan tag notch `[DROP] [TIMER] [REMIND] [MEDIA]`. Koordinat screenshot dipetakan ke DIP per monitor.
- **FR-1.4** Notch menampilkan fase: Mendengar → Melihat area → Berpikir → Menjawab, dengan maskot yang ekspresinya mengikuti fase.
- **FR-1.5** Umpan balik waktu: 1,5 dtk kalimat pengisi (hanya kalau audionya sudah di cache), 6 dtk timer, 20 dtk petunjuk `Esc`, 45 dtk giliran dibatalkan dengan pesan jelas.
- **FR-1.6** `Esc` selalu menghentikan suara, gambar, dan tugas. Tray → "Hentikan jawaban" setara.
- **FR-1.7** Riwayat percakapan 10 menit, jadi "yang tadi maksudnya apa?" dipahami.
- **FR-1.8** Pertanyaan bisa diketik (tray → Ketik pertanyaan); bar menutup sebelum screenshot.
- **Kriteria terima:** token pertama ±830 ms (terukur), suara pertama di bawah 3 dtk untuk jawaban pendek, visual sinkron dengan kalimat. Biaya terukur ±Rp 138 per pertanyaan dengan cache hangat.

### F2. Menandai sambil bicara
- **FR-2.1** Selama mendengar, gerakan mouse jadi tinta: lingkaran, garis bawah, atau menunjuk diam 0,6 dtk (cincin yang terisi). Overlay tetap click-through.
- **FR-2.2** Model menerima potongan area yang ditandai, deskripsi satu baris, dan nama elemen UI di bawahnya (UI Automation, `element_at` ±17 ms) bila tersedia dalam 800 ms.
- **FR-2.3** Saat dikirim, kursor aina terbang ke tanda.

### F3. Mode Guided
- **FR-3.1** `[TARGET]`/`[HOVER]` memulai panduan. Klik atau hover di dalam target membuat aina melihat ulang layar dan memberi langkah berikutnya, maksimal `maxGuidedRounds` (8).
- **FR-3.2** Klik di luar target mengakhiri panduan. Ada chime per langkah.

### F4. Dikte jadi catatan
- **FR-4.1** `Ctrl+Win` ditekan sekali untuk mulai dan sekali lagi untuk berhenti (toggle), maks. 10 menit. Tombol dikte tidak boleh sama dengan tombol bicara.
- **FR-4.2** Hasil tidak diketik ke aplikasi aktif. Ia disimpan sebagai Markdown di `Documents\aina\Dikte\<tanggal jam>.md` (PC lama: `Documents\Badi`), dengan front matter (waktu, durasi, bahasa, jumlah kata).
- **FR-4.3** "Rapikan hasil dikte" (default nyala) merapikan teks dengan Haiku. Di atas 120 kata, catatan diberi Ringkasan.
- **FR-4.4** Notch menampilkan "✓ Dikte tersimpan" dengan isi (bisa di-scroll) dan tombol Salin / Buka file / Tutup.

### F5. Ngobrol realtime dan "Hey aina"
- **FR-5.1** Mesin realtime OpenAI (`gpt-realtime-2.1-mini`) lewat WebRTC, opsional. Masuk dengan tahan (sekali tanya), ketuk (<350 ms, sesi), atau wake word. Secret 60 dtk dicetak oleh aina Cloud; renderer tidak pernah melihat key.
- **FR-5.2** Barge-in default `low` (hanya ucapan jelas yang memotong); bisa `off`. Sesi berakhir lewat "udah dulu", `Esc`, ketuk, atau diam 30 dtk.
- **FR-5.3** Hanya satu suara aina yang terdengar. Pertanyaan tentang layar diteruskan ke Claude (`look_at_screen`); tugas ke Kerjakan untukku dan Sprite.
- **FR-5.4** "Hey aina" dikenali offline, hanya saat aina diam, dan dijeda saat video atau lagu diputar.
- **FR-5.5** Cloud membatasi output realtime 1.500 token per balasan dan menagih dari usage yang dilaporkan.

### F6. Kerjakan untukku
- **FR-6.1** Izin sekali di awal untuk mengontrol mouse dan keyboard (`computerControl`). Saat ini dialog Windows OK/Batal.
- **FR-6.2** Loop agent (`brainModel` = `claude-sonnet-5-5`, maks. 60 langkah) dengan computer toolset di monitor yang sedang dilihat, UI Automation (`list_windows`, `get_window_state`, `click_element`, `set_value`, `focus_window`), `open_app`/`open_url`, `web_search`/`web_fetch`, ekstensi browser (opsional), file tools, edit tools, dan `save_file`.
- **FR-6.3** Riwayat append-only; screenshot lama dibersihkan di sisi server. Screenshot otomatis hanya 450 ms setelah aksi yang mengubah layar. Urutan melihat dari yang termurah: hasil tool → `get_window_state` → zoom → screenshot penuh.
- **FR-6.4** Sebelum mengirim, membeli, atau menghapus: konfirmasi (`confirm_with_user`). Pertanyaan muncul di notch (Boleh / Boleh semua / Jangan), diucapkan, dan bisa dijawab dengan suara, Enter, atau Esc. Jawaban ragu ("hmm, apa maksudnya?") **bukan** persetujuan.
- **FR-6.5** Setelah satu tool gagal, sisa tool di giliran itu dihentikan (HALT).
- **FR-6.6** Setiap tugas diukur: langkah, tool, screenshot, token, dan biaya; dipotong dari kredit.
- **FR-6.7** Ekstensi browser: WebSocket `127.0.0.1:47821`, Origin ekstensi + kode 6 digit (constant-time), dikunci setelah 10 salah per menit.
- **Kriteria terima:** Calculator 12×3 lewat UIA: 4 giliran, 17 dtk, $0,085 (terukur).

### F7. File tools dan izin
- **FR-7.1** Satu operasi per tool, diperiksa guard, tidak pernah menimpa, hasil diverifikasi di disk: `find_files`, `list_folder`, `move_paths`, `copy_paths`, `rename_path`, `create_folder`, `delete_paths` (Recycle Bin), `read_file`, `edit_file` (CRLF-aware), `write_file`.
- **FR-7.2** Selalu ditolak: Windows, Program Files, `AppData`, folder titik di home, drive atau folder utama itu sendiri, hapus permanen di luar folder kerja, perintah disk/registry/sistem.
- **FR-7.3** Perubahan ditanyakan lewat notch dan suara: "Aku mau hapus kalkulator.html dari folder aina ke Recycle Bin. Boleh?" Sprite menyebut namanya sendiri ("Filo mau memindahkan file di Unduhan. Boleh?").
- **FR-7.4** Setting "atur file tanpa bertanya" (`agentFileAccess`) melewati pertanyaan, tapi tidak melewati penolakan di FR-7.2 atau kunci fokus (FR-8.3).

### F8. Mode Fokus
- **FR-8.1** Fokus bisa dimulai lewat suara ("fokus di folder X", pencarian BFS kedalaman 5, bertanya kalau kandidat lebih dari satu), dengan men-drop folder ke notch, atau dari halaman Fokus di notch.
- **FR-8.2** Sesi fokus berisi nama, folder (opsional), deskripsi, target sesi, waktu mulai, dan kunci edit. Pomodoro yang terkait diberi label fokus dan dihitung ke durasinya. Riwayat 7 hari per proyek (maks. 50 sesi) di `config.json`.
- **FR-8.3** Semua prompt AI memuat ringkasan fokus. Dengan kunci menyala, perubahan file di luar folder fokus ditolak, sebelum `agentFileAccess` berlaku. Di dalam folder: membuat, mengedit, memindah tanpa izin; menghapus tetap bertanya.
- **FR-8.4** Review, diff, dan pencarian bug/celah keamanan bersifat read-only dan ditulis ke `badi-reports/<tanggal>-<jenis>.md`. "Perbaiki" bekerja dari laporan terbaru.
- **FR-8.5** Opsional: ringkasan saat selesai (selesai, tertunda, langkah berikutnya) dan mode jangan ganggu (menahan notifikasi selama fokus).

### F9. Sprite dan routine
- **FR-9.1** aina memunculkan Sprite untuk pekerjaan panjang (`[DELEGATE]`). Enam jenis: **Ciro** (coding), **Mino** (riset), **Routine** (jadwal), **Filo** (file), **Nilo** (email), **Daro** (data). Tugas kedua dari jenis yang sama jadi "Ciro 2".
- **FR-9.2** Maksimal 8 Sprite bekerja bersamaan; sisanya antre dengan pesan jelas. Satu pertanyaan izin ditampilkan pada satu waktu; menghentikan Sprite menjawab pertanyaannya.
- **FR-9.3** Mesin: Claude Code milik pengguna bila terpasang (CLI `stream-json`, config dir sementara, guard lewat hook `PreToolUse` yang fail-closed, tool aina lewat MCP bertoken per tugas), mesin bawaan aina sebagai cadangan, Codex opsional. `claude.exe` tidak dibundel.
- **FR-9.4** Workspace per jenis di `agents/<id>/` (memori `AGENTS.md`, chat maks. 50 pesan, `output/`, routine); `tmp/` dibersihkan tiap selesai tugas.
- **FR-9.5** Halaman Tugas di notch: maks. 2 tugas paling relevan (butuh kamu, gagal, berjalan, antre, selesai, dibatalkan), durasi (kuning setelah 15 menit), aktivitas terakhir, Lihat error, Stop (selalu ditanya dulu), Ulangi. Progress bar hanya untuk persentase sungguhan. Riwayat 8 tugas terakhir di memori.
- **FR-9.6** Selesai atau gagal memunculkan notice notch dan notifikasi Windows. Biaya tampil di bawah tiap balasan. Routine berjalan lagi setelah PC bangun.

### F10. Aplikasi kerja
- **FR-10.1** Lewat Composio di aina Cloud: key `ak_` hanya di server, setiap panggilan dipaksa ke `user_id` akun, auth config milik server.
- **FR-10.2** Hanya untuk akun berbayar (langganan aktif, atau pernah membayar dan masih punya kredit). Kredit Free saja tidak membukanya.
- **FR-10.3** Saat ini dibuka: Gmail, Google Calendar, Drive, Docs, Sheets, Tasks. Outlook, Teams, Notion, Linear, Jira, ClickUp, HubSpot, Salesforce, Dropbox, GitHub, Slack tampil "Segera" dan ditolak `connect_app`.
- **FR-10.4** Saklar per aplikasi di Settings. Menyalakan aplikasi yang belum tersambung membuka login; mematikan ditegakkan di kode; Putuskan menghapus sambungan.
- **FR-10.5** Membaca bebas. Mengirim, membuat, mengubah, menghapus ditanyakan dulu ("Aku mau kirim email (budi@x.com) di Gmail. Boleh?"), kecuali `agentFileAccess`.
- **FR-10.6** Upload Drive: file yang di-drop ke notch (drop = izin), notch mengecil jadi animasi awan + progress, notice berisi link. Maks. 100 MB per file.
- **FR-10.7** Composio yang tidak menjawab dalam 8 dtk tidak menggagalkan tugas.
- **Kriteria terima:** "cek 3 email terbaru" lewat Sprite $0,088 (terukur). Belum diuji nyata: kirim email, upload Drive.

### F11. Pengetahuan dan catatan
- **FR-11.1** File atau folder (teks, kode, Markdown, Word, PDF) yang di-drop ke notch masuk "pengetahuan". Hanya path-nya yang disimpan; isi diindeks di memori dan dibangun ulang saat file berubah. Batas: 1.500 file, 15 MB per file, kedalaman 5, folder seperti `node_modules`/`.git` dilewati.
- **FR-11.2** Tiap pertanyaan hanya membawa beberapa potongan yang cocok; aina menyebut nama file sumbernya. Realtime, Kerjakan untukku, dan Sprite mendapat daftar isinya.
- **FR-11.3** Drop folder proyek (ada `.git`/`package.json`) tanpa diminta menyalakan Fokus, dengan tombol "Jadikan pengetahuan".
- **FR-11.4** Notch → 📝 Catatan / 📚 Pengetahuan: daftar yang bisa di-scroll; × mengeluarkan dari pengetahuan tanpa menghapus file.

### F12. Notch pintar (gratis, tanpa akun)
- **FR-12.1** Notch selalu ada di tengah atas layar aktif. Diam: kecil. Ada tugas/timer/fokus: pil dengan titik berdenyut, cincin hitung mundur, atau ikon folder. Klik di luar notch tembus ke aplikasi.
- **FR-12.2** Hover membuka ringkasan yang melebar ke samping: jam, tanggal, dan cuaca di kiri; satu halaman per widget di kanan, semua setinggi sama. Pindah halaman lewat titik, panah, swipe touchpad, seret, atau scroll.
- **FR-12.3** **Home**: empat kartu (pengingat/salat berikutnya, tugas, timer/fokus, musik). **Cuaca** dari Open-Meteo tiap 30 menit; lokasi dari Windows Location API atau kota pilihan; posisi laptop tidak disimpan.
- **FR-12.4** **Timer dan pomodoro**: preset (25/5, 50/10 ×3, 15/3) dan preset sendiri, cincin berwarna per fase, Mulai/Jeda/Lewati/Ulang/Stop, statistik harian, notifikasi per fase.
- **FR-12.5** **Pusat Pengingat**: tampilan Agenda, Timeline, Kalender; pengingat lokal dengan ulang, tunda, selesai, terlewat (dibuang 14 hari setelah lewat). Google Calendar (kalau tersambung) maks. 10 kalender, di memori, sinkron saat dibuka dan tiap 10 menit; gagal sinkron tidak menghapus apa pun dan tidak dicoba otomatis sampai pengguna menekan Coba lagi.
- **FR-12.6** **Jadwal salat** (opsional): 8 waktu Kemenag dari myQuran (hanya id kota yang dikirim), cache sebulan di `config.json`, notifikasi 5 waktu wajib (default 10 menit sebelum).
- **FR-12.7** **Musik**: compact (equalizer), expanded saat lagu berganti (±4,5 dtk), interaktif (seek, volume, shuffle/repeat bila didukung, lirik dari LRCLIB yang bisa dimatikan, output suara). Lewat sesi media Windows (WinRT). Sengaja tanpa hotkey musik.
- **FR-12.8** **Layar penuh**: saat jendela di depan maximize atau layar penuh, notch di monitor itu sembunyi dan muncul setelah kursor diam ±1 dtk di tepi atas. Yang tetap tampil: giliran bicara, pertanyaan izin, zona drop, upload.
- **FR-12.9** **Drop**: zona drop muncul hanya saat sesuatu diseret ke notch. Folder → Fokus; dokumen → pengetahuan; file + "upload ke Drive" → Drive.
- **FR-12.10** Maks. 4 pintasan (prompt sekali klik), sisa kredit di peek, dan notice singkat yang tidak pernah menutupi pertanyaan izin.
- **FR-12.11** Maskot Rive (11 ekspresi), satu instance per overlay; berhenti menggambar saat diam atau tersembunyi.

### F13. Akun aina Cloud dan kredit
- **FR-13.1** Masuk dengan Google di browser; kembali ke app lewat `badi://` (state milik app, kode sekali pakai 5 menit, disimpan sebagai hash). Token perangkat `bct_…` di Windows Credential Manager.
- **FR-13.2** Tanpa akun, fitur AI menampilkan "Masuk ke aina Cloud dulu ya" dan Settings → Akun terbuka. Tidak ada kolom API key di Settings.
- **FR-13.3** Akun baru mendapat 10 kredit gratis sekali per PC (hash MachineGuid).
- **FR-13.4** Paket: Basic dan Pro (30 hari, sisa hangus), Top-Up Small/Medium/Large (tidak hangus), Enterprise (kontak). Kredit langganan dipakai dulu, lalu Top-Up. Daftar harga di tabel `plans`, bisa diubah tanpa rilis (BRD §7).
- **FR-13.5** Pembayaran Midtrans Snap dari app atau dari web (`/langganan`, tanpa app). Kartu kredit diperpanjang otomatis (coba ulang 3× dalam 3 hari masa tenggang) dan bisa dibatalkan, diaktifkan lagi, atau kartunya dihapus. QRIS, VA, e-wallet: sekali bayar.
- **FR-13.6** Biaya setiap panggilan dihitung dari usage sebenarnya dan dikonversi ke kredit (1 kredit = Rp 170 HPP). Peringatan di bawah 1/5 kredit siklus (atau di bawah 5 kredit tanpa langganan). Tugas yang sedang berjalan boleh selesai.
- **FR-13.7** Kredit habis: notch "Kredit habis" dengan tombol Beli kredit. Notch tetap gratis.
- **FR-13.8** Hapus akun dari Settings.
- **FR-13.9** Model dan suara dibatasi di server pada daftar yang dibayar aina Cloud.

### F14. Settings, onboarding, i18n
- **FR-14.1** Settings terbuka pada pemakaian pertama. Halaman: Akun, Suara (mesin, suara, Hey aina, barge-in), Tombol, Aplikasi kerja, Pomodoro, Pengingat, Jadwal salat, Lokasi & cuaca, Fokus, izin file, Jalan saat Windows mulai.
- **FR-14.2** Perubahan berlaku tanpa restart, termasuk kombinasi tombol bicara.
- **FR-14.3** Semua teks app dan kalimat yang diucapkan tersedia dalam bahasa Indonesia dan Inggris. `auto` mengikuti bahasa ucapan terakhir.
- **FR-14.4** Sapaan saat app dibuka mengikuti waktu dan kebiasaan, tanpa menyebut nama tombol keyboard.
- **FR-14.5** Tray: status, folder fokus, tombol bicara, Ketik pertanyaan, Sprite, Hentikan jawaban, Kirim masukan, Buka log, Keluar. Single-instance.

### F15. Instalasi dan distribusi
- **FR-15.1** Installer NSIS per pengguna (tanpa admin) dengan halaman ketentuan, gambar sidebar/header, dan bootstrapper WebView2. Diunduh dari `/download` (R2).
- **FR-15.2** Paket MSIX untuk Microsoft Store (`RagamKarya.Aina`, Ragam Karya): `badi://` dan StartupTask dideklarasikan di manifest.
- **FR-15.3** Belum ada: code signing, updater otomatis (BRD §11).

## 6. Model izin

| Aksi | Default | Dengan "atur tanpa bertanya" | Di folder fokus (kunci menyala) |
|---|---|---|---|
| Membaca file atau data aplikasi | Bebas | Bebas | Bebas |
| Membuat, mengedit, memindah file | Tanya | Bebas | Bebas di dalam folder, ditolak di luar |
| Menghapus file (ke Recycle Bin) | Tanya | Bebas | Tanya |
| Hapus permanen di luar folder kerja | Ditolak | Ditolak | Ditolak |
| Folder sistem, AppData, folder titik, drive | Ditolak | Ditolak | Ditolak |
| Mengirim, membuat, mengubah di aplikasi kerja | Tanya | Bebas | Tanya |
| Upload file yang di-drop ke notch | Bebas (drop = izin) | Bebas | Bebas |
| Aplikasi yang saklarnya mati | Ditolak | Ditolak | Ditolak |
| Mengontrol mouse dan keyboard | Izin sekali | Izin sekali | Izin sekali |
| Mengirim atau membeli di browser/desktop | Konfirmasi | Konfirmasi | Konfirmasi |
| Jawaban yang tidak jelas | Dianggap "jangan" | — | — |

## 7. Kebutuhan non-fungsional

| Area | Kebutuhan | Kondisi terukur |
|---|---|---|
| **Privasi** | Di disk hanya `config.json` dan hasil yang diminta (catatan dikte, `save_file`, workspace Sprite, laporan fokus). Screenshot, rekaman, audio TTS, indeks pengetahuan, data kalender, cuaca, musik hanya di memori. Pertanyaan, suara, dan screenshot melewati aina Cloud tanpa disimpan. `badi.log` hanya error teknis. | Kebijakan privasi di `/privasi` |
| **Keamanan** | Tidak ada key penyedia di PC. Token di Credential Manager. CSP Tauri ketat + `freezePrototype`. Allowlist IPC di `shim.js` dan `ipc.rs`. Guard tool + hook fail-closed. Bridge hanya `127.0.0.1`. Overlay content-protected. | [hardening.md](hardening.md) |
| **Performa** | Tidak ada loop frame saat diam. Hover notch lewat hook mouse, bukan polling. | RAM 400 MB (−28% vs v1), CPU diam 0,04%, maskot idle 0,02% CPU / 0% GPU |
| **Latensi** | Token pertama < 1 dtk; suara pertama < 3 dtk. TTS hedged (ElevenLabs → OpenAI → suara Windows). | Token pertama 830 ms; TTS 0,8 dtk; STT 1,9 dtk |
| **Biaya** | Companion `claude-opus-5` effort `low`; Brain dan Sprite `claude-sonnet-5-5`; prompt cache + prewarm; pembersihan screenshot di server. Setiap panggilan dicatat. | Tanya ±Rp 138 (cache hangat) |
| **Ukuran** | Exe kecil, UI tertanam. | `badi.exe` 9,2 MB |
| **Kompatibilitas** | Windows 10/11 x64, WebView2. Input ke aplikasi admin butuh aina dijalankan sebagai admin. | Belum diuji 2 monitor dengan DPI berbeda |
| **Keandalan** | Fallback suara, fallback model di server, mesin Sprite cadangan, Composio boleh gagal, overlay dibuat ulang saat monitor berubah. Pesan jelas saat offline. | — |
| **Aksesibilitas** | "Kurangi gerakan" menghentikan animasi cuaca. Semua aksi notch bisa dilakukan lewat suara. | — |

## 8. Ketergantungan eksternal

| Layanan | Untuk | Wajib |
|---|---|---|
| aina Cloud (Cloudflare Workers + D1 + R2) | Akun, kredit, proxy AI dan Composio, unduhan | Ya, untuk fitur AI |
| Anthropic (Claude) | Tanya layar, Kerjakan untukku, Sprite, rapikan dikte | Ya |
| OpenAI | STT, realtime, suara alternatif | Ya, untuk STT |
| ElevenLabs | Suara default | Tidak (fallback) |
| Composio | Aplikasi kerja, Google Calendar di notch, upload Drive | Tidak |
| Google OAuth | Login akun | Ya |
| Midtrans | Pembayaran | Ya, untuk berbayar |
| Open-Meteo, BigDataCloud, myQuran, LRCLIB | Cuaca, nama lokasi, salat, lirik | Tidak (gratis, tanpa key) |
| Claude Code milik pengguna | Mesin Sprite | Tidak |
| Chrome/Edge + ekstensi aina | Kontrol browser | Tidak |
| WebView2 Runtime | Tampilan app | Ya (bootstrapper di installer) |

## 9. Metrik keberhasilan

| Metrik | Cara ukur | Target awal (usulan) |
|---|---|---|
| Waktu sampai suara pertama | `BADI_TIMING` | < 3 dtk untuk jawaban pendek |
| Biaya per tanya / per Kerjakan | Pencatatan biaya di aina Cloud | ≤ 1 kredit / median ≤ 10 kredit |
| Aktivasi | Akun baru yang bertanya ≥ 3× di hari pertama (D1 Cloud) | ≥ 50% |
| Konversi Free → berbayar | Akun Free yang membeli dalam 14 hari | ≥ 5% |
| Retensi berbayar | Langganan yang diperpanjang | ≥ 60% di bulan ke-2 |
| Tingkat "jangan" | Pertanyaan izin yang ditolak | Dipantau; tinggi = aina terlalu agresif |
| Tugas gagal/dibatalkan | Log tugas | Turun dari rilis ke rilis |
| Pemakaian notch saja | Belum bisa diukur tanpa telemetry | Keputusan: telemetry opt-in atau survei |

## 10. Batasan saat ini
- Kerjakan untukku hanya melihat satu monitor (tempat kursor saat tugas mulai).
- Ekstensi browser belum mendukung upload file, unduhan, dan eksekusi JavaScript.
- "Hey aina" memakai pengenal suara bahasa Inggris bawaan Windows.
- Aplikasi kerja baru 6 aplikasi Google; 11 lainnya "Segera".
- Midtrans masih sandbox; simpan kartu perlu diaktifkan Midtrans untuk production.
- Izin kontrol komputer masih MessageBox Windows, belum tombol "Izinkan / Jangan sekarang".
- Belum diuji fisik: dua monitor beda DPI, mic sungguhan oleh penguji, routine melewati sleep, ekstensi Chrome sungguhan, kirim email dan upload Drive nyata.

## 11. Rencana rilis

| Tahap | Isi | Gerbang |
|---|---|---|
| **Beta tertutup** (sekarang) | 10–20 penguji, [QA-paritas.md](QA-paritas.md), [BETA.md](BETA.md) | Semua baris QA ✅/⚠️, tanpa ❌ P0 |
| **Rilis publik 1.0** | Code signing, updater Tauri, Midtrans production, review hukum privasi/ketentuan (UU PDP), pentest Worker | BRD §10 |
| **1.x** | Aplikasi kerja non-Google, dialog izin kontrol komputer versi notch, Brain multi-monitor, fitur ekstensi yang kurang | — |

## 12. Visi (belum dibangun)
Ponsel (Android/iOS), macOS, Linux, wearable (perintah suara singkat; kacamata yang melihat dan menunjuk), TV (pengaturan dipandu suara). Companion dan lapisan izinnya sama di setiap layar.

## 13. Pesaing
Ringkasannya ada di [BRD §12](BRD.md#12-pesaing). Implikasi produk: pertahankan tiga pembeda (melihat layar, menunjuk sambil bicara, lapisan izin yang ditegakkan di kode), jaga keunggulan di Windows, dan buat bahasa Indonesia terasa paling natural. Notch gratis adalah pintu masuk yang tidak dimiliki pesaing.

## 14. Pertanyaan terbuka
1. Bagaimana mengukur aktivasi notch dan retensi tanpa melanggar prinsip privasi: telemetry opt-in atau survei?
2. Apakah kuota Basic (85 kredit ≈ 85 tanya per bulan) cukup terasa bernilai, atau perlu dinaikkan? (BRD §7)
3. Urutan membuka aplikasi kerja non-Google, dan apakah perlu verifikasi OAuth sendiri (CASA untuk Gmail) atau tetap memakai login Composio?
4. Kapan dialog izin kontrol komputer dipindah ke notch?
5. Apakah build Store dan build NSIS memakai kanal update yang sama?
