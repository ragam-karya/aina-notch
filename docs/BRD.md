# BRD: aina

| | |
|---|---|
| **Produk** | aina (dulu B.A.D.I. / "Badi"), asisten AI untuk Windows yang paham layar |
| **Versi produk** | 1.0.0 (Rust + Tauri 2) |
| **Dokumen** | Business Requirements Document |
| **Status** | Draft untuk review |
| **Pemilik** | Faisal (Ragam Karya) |
| **Tanggal** | 3 November 2026 |
| **Terkait** | [PRD.md](PRD.md), [../../BRANDING.md](../../BRANDING.md), [../cloud/README.md](../cloud/README.md), [hardening.md](hardening.md), BRD v1: [../../docs/BRD.md](../../docs/BRD.md) |

> Dokumen ini menggantikan BRD v1 (19 Okt 2026). Bedanya: model distribusi sudah diputuskan (cloud-only dengan kredit, tanpa BYOK), aina Cloud sudah production, dan app tersedia lewat web dan Microsoft Store.
>
> Angka yang **terukur** diberi tanda. Angka pasar, konversi, dan pola pemakaian adalah **asumsi** yang harus divalidasi selama beta. Kurs: **Rp 18.000 per US$**.

---

## 1. Latar belakang

Pekerja di depan komputer menghabiskan banyak waktu untuk tiga hal: mencari cara melakukan sesuatu di aplikasi, berpindah antara pekerjaan dan jendela chat AI, dan mengerjakan tugas kecil yang berulang (email, jadwal, file, dokumen).

Asisten AI yang ada umumnya berbentuk kotak chat. Asisten itu tidak melihat layar, tidak bisa menunjuk, dan tidak bisa bertindak dengan aman di komputer pengguna.

aina mengisi celah ini: teman di sebelah kursor yang melihat, menunjuk, memandu, dan mengerjakan. Setiap perubahan seizin pengguna, dan tidak ada data pribadi yang disimpan di PC maupun di server.

Versi 1.0.0 menulis ulang app dari Electron ke Rust + Tauri (12× lebih kecil, RAM −28%, CPU diam 8× lebih hemat), memindahkan semua key AI ke **aina Cloud**, dan menambah notch pintar yang gratis sebagai pintu masuk.

## 2. Tujuan bisnis

| ID | Tujuan | Indikator |
|---|---|---|
| BO-1 | Membuktikan nilai aina untuk pekerja kantor dan developer di Windows | Beta tertutup 10–20 orang selesai; ≥ 50% akun baru bertanya ≥ 3× di hari pertama |
| BO-2 | Model bisnis kredit yang berkelanjutan | Margin kotor ≥ 60% dari uang masuk per paket (bagian 7); konversi Free → berbayar ≥ 5% |
| BO-3 | Kepercayaan sebagai pembeda | Nol insiden data; semua perubahan seizin pengguna; kebijakan privasi dan ketentuan sudah direview hukum |
| BO-4 | Siap didistribusikan ke publik | Installer dan exe bertanda tangan, updater otomatis, Midtrans production, listing Microsoft Store live |
| BO-5 | Onboarding tanpa hambatan teknis | Pasang → masuk Google → pertanyaan pertama dalam < 3 menit, tanpa API key |

## 3. Pemangku kepentingan

| Pihak | Peran | Kepentingan |
|---|---|---|
| Pemilik produk (Faisal, Ragam Karya) | Keputusan produk, bisnis, dan pengembang terdaftar di Store | Arah, prioritas, biaya |
| Pengguna akhir | Memakai aina; membayar kredit | Produktivitas, kemudahan, privasi, harga |
| Penguji beta | Uji paritas dan umpan balik | Panduan jelas ([BETA.md](BETA.md)), kredit uji |
| Anthropic, OpenAI, ElevenLabs | Pemroses data (*data processor*) model dan suara | Kepatuhan usage policy |
| Composio | Perantara aplikasi kerja, penyimpan token OAuth | Kepatuhan, kuota |
| Cloudflare | Hosting aina Cloud (Workers, D1, R2) | Batas CPU dan body request |
| Midtrans | Gerbang pembayaran | Kepatuhan merchant, tokenisasi kartu |
| Google | Login akun (OAuth `openid email`), platform aplikasi kerja | Verifikasi aplikasi, kebijakan scope |
| Microsoft | Microsoft Store, Windows, WebView2 | Kebijakan Store, sertifikasi paket |

## 4. Ruang lingkup bisnis

**Termasuk:**
- App desktop Windows 10/11 dengan semua fitur di [PRD §5](PRD.md#5-ruang-lingkup-fitur).
- aina Cloud: akun Google, kredit, pembayaran Midtrans, proxy AI dan Composio, halaman web langganan, unduhan installer, API admin.
- Distribusi lewat web (installer NSIS) dan Microsoft Store (MSIX).
- Ekstensi browser untuk Chrome dan Edge.
- Aplikasi kerja Google (6 aplikasi) untuk akun berbayar.
- Bahasa Indonesia dan Inggris.

**Tidak termasuk (fase ini):**
- macOS, Linux, ponsel, wearable, TV.
- BYOK di build publik.
- Fitur enterprise (SSO, admin console, kebijakan organisasi). Paket Enterprise dilayani manual lewat kontak.
- Aplikasi kerja non-Google (sudah ada di kode, ditandai "Segera").

## 5. Kebutuhan bisnis

| ID | Kebutuhan | Prioritas | Turunan di PRD |
|---|---|---|---|
| BR-1 | Pengguna mendapat jawaban tentang layarnya lewat suara dan visual tanpa pindah aplikasi | Wajib | F1–F3 |
| BR-2 | Pengguna bisa mendelegasikan tugas di komputer, file, dan aplikasi kerja lewat suara | Wajib | F6, F7, F9, F10 |
| BR-3 | Tidak ada perubahan atau pengiriman tanpa izin, kecuali diizinkan di Settings | Wajib | F7, F10, PRD §6 |
| BR-4 | Tidak ada data pribadi di PC selain pengaturan dan hasil yang diminta; server tidak menyimpan isi percakapan | Wajib | PRD §7 |
| BR-5 | Pemakaian pertama tanpa API key: masuk dengan Google, dapat kredit coba | Wajib | F13 |
| BR-6 | Setiap panggilan AI dihitung dari biaya sebenarnya dan dipotong dari kredit di server, bukan di app | Wajib | F13 |
| BR-7 | Pengguna bisa membeli kredit dari app maupun dari web, dan membatalkan langganan sendiri | Wajib | F13 |
| BR-8 | Harga, kuota, dan paket bisa diubah tanpa rilis app | Wajib | Tabel `plans`, API admin |
| BR-9 | Notch berguna tanpa akun, sebagai pintu masuk gratis | Penting | F12 |
| BR-10 | Tidak ada key atau token milik pemilik produk di installer atau repo | Wajib | Cloud-only (commit `5a2db01`) |
| BR-11 | Teks dan suara konsisten dengan brand (aina, huruf kecil) dalam dua bahasa | Wajib | F14 |
| BR-12 | Distribusi resmi lewat Microsoft Store dan web | Penting | F15 |

## 6. Struktur biaya

### Biaya variabel (ditanggung aina Cloud, dibebankan ke kredit)

| Komponen | Model atau layanan | Harga satuan |
|---|---|---|
| Tanya layar | Claude `claude-opus-5`, effort `low` | $5 input, $25 output, $0,5 baca cache, $6,25 tulis cache per juta token |
| Kerjakan untukku, Sprite | Claude `claude-sonnet-5-5` | $2 input, $10 output, $0,2 baca cache, $2,5 tulis cache per juta token |
| Rapikan dikte | Claude Haiku | Kecil |
| Suara ke teks | OpenAI `gpt-4o-mini-transcribe` | ±$0,003 per menit |
| Suara aina (default) | ElevenLabs `eleven_flash_v2_5` | ±$0,05 per 1.000 karakter |
| Suara alternatif | OpenAI `gpt-4o-mini-tts` | ±$0,004 per jawaban |
| Realtime (opsional) | OpenAI `gpt-realtime-2.1-mini`, output maks. 1.500 token per balasan | Per token audio |
| Aplikasi kerja | Composio | ±$0,0003 per panggilan tool |

Harga OpenAI, ElevenLabs, dan Composio dari sumber sekunder; cek ulang di halaman harga resmi sebelum mengubah tabel `plans`.

### Biaya per pemakaian

| Pemakaian | Biaya modal | Dalam kredit (1 kredit = Rp 170) | Dasar |
|---|---|---|---|
| 1× tanya layar (cache hangat) | Rp ±138 | ±1 | **Terukur** (`BADI_DEMO=ask`, Fase 5) |
| 1× jawaban realtime | Rp ±300 | ±1,8 | [BETA.md](BETA.md) |
| 1 menit dikte | Rp ±10 | ±0,06 | [BETA.md](BETA.md) |
| 1× Kerjakan untukku | Rp ±1–5 rb | ±6–30 | Calculator 12×3: $0,085 (**terukur**); tugas file $0,03–0,06 (terukur di v1) |
| 1× tugas Sprite | Rp ±1–10 rb | ±6–60 | "Cek 3 email" $0,088, tulis file $0,041 (**terukur**); tugas panjang taksiran |
| Kredit Free per PC baru | Rp 1.700 | 10 | Subsidi marketing |

Temuan dari data Claude Console (v1, 16–18 Okt 2026, tetap berlaku): **57% biaya berasal dari menulis cache**, terutama di Brain dan agent. Menghemat screenshot dan hasil tool lebih berpengaruh daripada memperpanjang umur cache.

### Biaya tetap per bulan

| Komponen | Pilihan | Per bulan |
|---|---|---|
| aina Cloud | Cloudflare Workers Paid ($5; D1 dan R2 masih dalam kuota awal) | Rp 90 rb |
| Domain | .com / .id | Rp 25 rb |
| Composio | Paket Pro ($29) | Rp 520 rb |
| Code signing | Azure Trusted Signing ±$10/bulan, atau sertifikat OV $200–400/tahun | Rp 180–600 rb |
| Email kantor | Google Workspace | Rp 110 rb |
| Pemantauan error, email transaksional | Paket gratis dulu | Rp 0–850 rb |
| Akuntan dan pajak | Jasa ringan | Rp 500 rb–1 jt |
| **Total** | | **±Rp 1,4–3,2 juta** |

Dibanding BRD v1 (Rp 2–4,5 juta), Supabase tidak dipakai lagi karena akun dan saldo ada di D1.

### Biaya sekali bayar

| Komponen | Biaya |
|---|---|
| Pendirian PT perorangan (Ragam Karya) | Rp 1–5 juta |
| Pendaftaran merek "aina" di DJKI | ±Rp 1,8 juta per kelas (cek ketersediaan nama dulu) |
| Akun developer Microsoft Store | Cek biaya terbaru (akun perusahaan berbayar sekali) |
| Pendaftaran PSE Komdigi | Gratis, wajib |
| Review hukum kebijakan privasi dan ketentuan | Rp 0–5 juta |
| Pentest aina Cloud oleh pihak lain | Rp 5–20 juta (perkiraan) |
| **Total** | **±Rp 8–32 juta** |

## 7. Model bisnis: kredit

**Keputusan (Okt 2026):** aina hanya jalan lewat aina Cloud (opsi B di BRD v1). BYOK dihapus dari build publik. Alasannya: onboarding satu klik, key tidak pernah ada di PC, dan pemakaian bisa dihitung dan dibatasi di server.

### Paket

Sumber: `cloud/migrations/0003_credits.sql`, bisa diubah lewat `PUT /admin/plans` tanpa rilis.

| Paket | Harga | Kredit | Rp/kredit | Catatan |
|---|---|---|---|---|
| Free | Rp 0 | 10, sekali per PC | — | Subsidi marketing |
| Basic | Rp 49 rb / 30 hari | 85 | 576 | Sisa hangus di akhir siklus |
| Pro ("Best Value") | Rp 129 rb / 30 hari | 225 | 573 | Sisa hangus di akhir siklus |
| Top-Up Small | Rp 49 rb | 65 | 754 | Tidak hangus |
| Top-Up Medium | Rp 149 rb | 210 | 710 | Decoy |
| Top-Up Large ("Paling hemat") | Rp 199 rb | 300 | 663 | Target |
| Enterprise | Custom | Custom | — | Lewat kontak |

Aturan: kredit langganan dipakai dulu, lalu Top-Up. Langganan kartu kredit diperpanjang otomatis (coba ulang 3× dalam 3 hari masa tenggang); QRIS, VA, e-wallet, dan Top-Up sekali bayar. Aplikasi kerja hanya untuk akun yang pernah membayar. Peringatan di bawah 1/5 kredit siklus; tugas yang sedang berjalan boleh selesai.

### Margin per paket

Uang masuk = harga setelah PPN dan biaya pembayaran (±15%). Biaya AI dihitung untuk kasus terburuk: **semua kredit terpakai**. Kredit langganan yang hangus menaikkan margin sebenarnya.

| Paket | Harga | Uang masuk | Biaya AI maks. | Laba kotor | Margin |
|---|---|---|---|---|---|
| Basic | Rp 49.000 | Rp 41.650 | Rp 14.450 | Rp 27.200 | 65% |
| Pro | Rp 129.000 | Rp 109.650 | Rp 38.250 | Rp 71.400 | 65% |
| Top-Up Small | Rp 49.000 | Rp 41.650 | Rp 11.050 | Rp 30.600 | 73% |
| Top-Up Medium | Rp 149.000 | Rp 126.650 | Rp 35.700 | Rp 90.950 | 72% |
| Top-Up Large | Rp 199.000 | Rp 169.150 | Rp 51.000 | Rp 118.150 | 70% |

Ini jauh lebih sehat daripada usulan langganan v1 (26–38%), karena setiap kredit menanggung biayanya sendiri dan tidak ada pemakaian tanpa batas.

### Pemakaian per profil (taksiran, perlu divalidasi)

Per bulan, 22 hari kerja. Kerjakan ±15 kredit, Sprite ±30 kredit (nilai tengah).

| Profil | Pemakaian | Kredit | Biaya modal | Kalau dibeli di harga Pro (Rp 573/kredit) |
|---|---|---|---|---|
| Tanya saja | 5 tanya/hari | ±110 | Rp ±19 rb | Basic + sedikit Top-Up, atau Pro |
| Ringan | 5 tanya/hari, 10 Kerjakan, 2 Sprite | ±320 | Rp ±54 rb | ±Rp 183 rb |
| Sedang | 15 tanya/hari, 40 Kerjakan, 10 Sprite | ±1.230 | Rp ±209 rb | ±Rp 705 rb |

**Implikasi:**
- Pengguna yang terutama bertanya tentang layar cocok di Basic atau Pro.
- Pengguna yang banyak memakai Kerjakan dan Sprite akan membayar jauh di atas harga paket. Ada risiko mereka merasa mahal dan berhenti (bagian 9).
- Pro hampir tidak lebih murah per kredit daripada Basic (Rp 573 vs Rp 576), jadi label "Best Value" lemah. Pertimbangkan Pro di ±Rp 500/kredit (mis. 260 kredit).
- Cara menurunkan biaya Kerjakan dan Sprite: lebih sedikit screenshot, lebih banyak UIA dan hasil tool, dan cache yang lebih jarang ditulis ulang.

### Titik impas

Asumsi: rata-rata pembayaran Rp 100 rb per pelanggan berbayar per bulan (campuran Basic, Pro, dan Top-Up), margin kotor 65% dari uang masuk (Rp 85 rb), jadi laba kotor ±Rp 55 rb.

| Pelanggan berbayar | Omzet/bulan | Laba kotor | Biaya tetap | Laba bersih |
|---|---|---|---|---|
| 30 | Rp 3 jt | Rp 1,7 jt | Rp 3,2 jt | −Rp 1,5 jt |
| ±60 | Rp 6 jt | Rp 3,3 jt | Rp 3,2 jt | ±Rp 0 (titik impas) |
| 300 | Rp 30 jt | Rp 16,5 jt | Rp 3,5 jt | Rp 13 jt |
| 1.000 | Rp 100 jt | Rp 55 jt | Rp 6 jt | Rp 49 jt |
| 5.000 | Rp 500 jt | Rp 275 jt | Rp 12 jt | Rp 263 jt |

Belum termasuk gaji (dengan gaji pemilik Rp 10 juta, titik impas naik ke ±240 pelanggan), marketing, dan pajak (PPh final UMKM 0,5% dari omzet). Kredit Free hanya Rp 1.700 per PC baru, jadi biaya akuisisi lewat coba gratis sangat kecil.

## 8. Distribusi

| Kanal | Status | Catatan |
|---|---|---|
| **Web** | Ada | Landing (`aina-landing`) → `/langganan` → login Google → Midtrans → `/terima-kasih` → `/download` (installer di R2) atau `badi://open` |
| **Microsoft Store** | Paket MSIX dan listing siap ([store/listing.md](../store/listing.md)) | `RagamKarya.Aina`, pengembang Ragam Karya. Store menangani tanda tangan dan update untuk kanal ini. Perlu dicek: aturan Store untuk pembayaran di luar Store |
| **Beta tertutup** | Bisa dinyalakan lagi (`BETA = "1"`) | Hanya email undangan; diatur lewat API admin |

## 9. Kepatuhan dan legal

| Area | Kebutuhan |
|---|---|
| **UU PDP (UU No. 27 Tahun 2022)** | Dengan aina Cloud, Ragam Karya menjadi pengendali data untuk email, paket, pembayaran, dan catatan biaya. Isi pertanyaan, suara, dan screenshot hanya lewat ke pemroses (Anthropic, OpenAI, ElevenLabs, Composio) dan tidak disimpan. Wajib: persetujuan, transparansi pemroses, hak hapus (sudah ada: hapus akun), review hukum halaman `/privasi` dan `/ketentuan`. |
| **PSE Komdigi** | Pendaftaran PSE lingkup privat untuk layanan digital berbayar. |
| **Pembayaran** | Midtrans production; tokenisasi kartu harus diaktifkan Midtrans. Notifikasi dicek dengan tanda tangan SHA-512. PPN atas layanan digital. |
| **Microsoft Store** | Kebijakan konten dan pembayaran Store; deklarasi privasi; deskripsi hanya fitur yang sudah ada. |
| **OAuth Google** | Login akun hanya `openid email` (tanpa verifikasi). Aplikasi kerja lewat Composio; kalau login atas nama aina, scope Gmail tergolong *restricted* dan butuh verifikasi + CASA. |
| **Penyedia AI** | Usage policy masing-masing; data API tidak dipakai untuk melatih model sesuai ketentuan mereka. |
| **Lisensi** | `claude.exe` tidak dibundel. Ripgrep MIT. Lirik dari LRCLIB hanya ditampilkan, tidak disimpan. |
| **Merek** | Nama "aina" perlu dicek dan didaftarkan di DJKI. Tidak memakai nama atau aset produk lain di materi publik. |

## 10. Kriteria keberhasilan fase ini (sebelum rilis publik)
1. Beta tertutup 10–20 orang menjalankan [QA-paritas.md](QA-paritas.md) tanpa ❌ di fitur P0.
2. Installer dan `badi.exe` bertanda tangan; updater Tauri aktif (endpoint dari Worker yang sama).
3. Midtrans production dengan simpan kartu aktif.
4. Pentest aina Cloud oleh pihak lain dan review hukum privasi/ketentuan selesai.
5. Uji fisik: dua monitor beda DPI, mic sungguhan, routine melewati sleep.
6. Listing Microsoft Store lolos sertifikasi.
7. Tidak ada key atau token pemilik produk di repo maupun installer (sudah, cloud-only).

## 11. Risiko

| Risiko | Dampak | Kemungkinan | Mitigasi |
|---|---|---|---|
| Kerjakan dan Sprite terasa mahal dalam kredit | Tinggi | Sedang | Hemat screenshot, tampilkan perkiraan kredit sebelum tugas panjang, tinjau ulang harga per kredit |
| Konversi Free → berbayar rendah | Tinggi | Sedang | Notch gratis untuk retensi, pintasan dan Home yang mengingatkan nilai AI, ukur aktivasi |
| aina melakukan aksi yang tidak diinginkan | Tinggi | Rendah | Guard tool, izin lewat suara, jawaban ragu = "jangan", Recycle Bin, kunci fokus, saklar per aplikasi, `Esc` |
| SmartScreen menakuti pengguna (installer belum bertanda tangan) | Tinggi | Tinggi (sekarang) | Code signing sebelum rilis publik; Store sebagai kanal utama |
| aina Cloud down atau diserang | Tinggi | Rendah | Cloudflare, token di-hash, pentest; notch tetap jalan offline |
| Penyalahgunaan kredit Free (banyak akun) | Sedang | Sedang | Free sekali per PC (hash MachineGuid), hanya 10 kredit |
| Biaya penyedia AI naik | Sedang | Sedang | Harga per kredit di tabel `plans`, bisa disesuaikan tanpa rilis |
| Ketergantungan pada Composio | Sedang | Sedang | Abstraksi di `badi-apps`; integrasi langsung untuk Google bisa ditambahkan |
| Midtrans menunda simpan kartu | Sedang | Sedang | QRIS/VA/e-wallet sekali bayar tetap jalan |
| Latensi atau gangguan penyedia suara | Sedang | Sedang | Request hedged, fallback ElevenLabs → OpenAI → suara Windows |
| Nama "aina" bentrok dengan merek lain | Sedang | Belum dicek | Cek DJKI dan Store sebelum marketing besar |

## 12. Pesaing

Kategori ini sudah didanai investor di luar negeri. Di Indonesia belum ada pemain lokal (riset founder, Oktober 2026). Angka di bawah sebagian besar dilaporkan sendiri dan belum diaudit.

| | **aina** | Cluely | HeyClicky | Jarvis | Kata.ai | Sahabat-AI |
|---|---|---|---|---|---|---|
| Asal | **Indonesia** | AS | AS (YC Spring 2026) | Rumania | Indonesia | Indonesia |
| Jenis | **Asisten yang paham layar** | Asisten AI desktop | Asisten layar untuk Mac | Asisten AI desktop | Chatbot perusahaan | LLM bahasa Indonesia |
| Melihat layar | **Ya** | Ya | Ya | Ya | Tidak | Tidak |
| Jalan di Windows | **Ya** | Belum dicek | Tidak (Windows daftar tunggu) | Ya | Bukan app desktop | Bukan app desktop |
| Dibuat untuk Indonesia | **Ya** | Tidak | Tidak | Tidak | Ya | Ya |
| Menunjuk tombol sambil bicara | **Ya** | Belum dicek | Belum dicek | Belum dicek | Tidak | Tidak |
| Bertanya sebelum bertindak | **Ya, lewat suara** | Belum dicek | Belum dicek | Belum dicek | Tidak relevan | Tidak relevan |
| Harga | **Gratis (notch) + kredit mulai Rp 49 rb** | — | Gratis–US$100/bln | US$16–32/bln | Enterprise | — |
| Pendanaan | Pre-seed (sedang galang) | ±US$20,3 juta; ARR US$5,2 juta (Jun 2025) | ±US$0,5 juta | Tidak diungkap | ≥US$3,5 juta | Korporat (GoTo + Indosat) |

"Belum dicek" berarti tidak bisa diverifikasi dari sumber publik; jangan diklaim tidak ada.

**Posisi aina:** satu-satunya yang sekaligus melihat layar, jalan di Windows (±80% desktop di Indonesia), dan dibuat untuk Indonesia; menunjuk sambil bicara; lapisan izin ditegakkan di kode; notch gratis sebagai pintu masuk; harga dalam rupiah dengan QRIS dan e-wallet.

Sumber: Cluely ([Wikipedia](https://en.wikipedia.org/wiki/Cluely), [Latka](https://getlatka.com/companies/cluely)), HeyClicky ([heyclicky.com](https://www.heyclicky.com/), [Zeitgeist](https://www.zeitgeist.bot/company/heyclicky)), Jarvis ([getjarvis.eu](https://www.getjarvis.eu/)), Kata.ai ([Yespress](https://yespress.io/kata-ai)), Sahabat-AI ([Analytics Insight](https://www.analyticsinsight.net/news/sahabat-ai-empowering-indonesia-with-ai-in-local-languages)). Detail lengkap di [BRD v1 §13](../../docs/BRD.md).

## 13. Estimasi pasar dan pendapatan (Indonesia)

### Ukuran pasar
| Langkah | Angka | Sumber atau asumsi |
|---|---|---|
| Penduduk bekerja | 146,54 juta | BPS Sakernas, Agustus 2025 |
| × buruh, karyawan, pegawai (38,74%) | ±56,8 juta | BPS, Agustus 2025 |
| × punya komputer (18,52%) | ±10,5 juta | BPS Statistik Telekomunikasi 2024 (proksi rumah tangga) |
| × memakai Windows (80,56%) | **±8,5 juta = SAM** | StatCounter, desktop Indonesia, Agustus 2026 |

### Skenario pendapatan
Asumsi rata-rata Rp 100 rb per pelanggan berbayar per bulan (Rp 1,2 juta per tahun).

| Skenario | Berbayar dari SAM | Pelanggan berbayar | ARR | Laba kotor/tahun (55%) |
|---|---|---|---|---|
| Konservatif | 0,25% | ±21.000 | ±Rp 25 miliar (±US$1,4 juta) | ±Rp 14 miliar |
| Dasar | 1% | ±85.000 | ±Rp 102 miliar (±US$5,7 juta) | ±Rp 56 miliar |
| Optimis | 2% | ±170.000 | ±Rp 204 miliar (±US$11,3 juta) | ±Rp 112 miliar |

Pembanding: Cluely melaporkan ARR US$5,2 juta dengan pendanaan US$20,3 juta.

**Yang perlu divalidasi selama beta:**
- Kredit per Kerjakan dan per Sprite di pemakaian nyata (bukan demo).
- Rata-rata pembayaran per pelanggan dan campuran langganan vs Top-Up.
- Konversi Free → berbayar, dan peran notch gratis dalam retensi.
- Kesediaan membayar Rp 49–129 rb per bulan.

Sumber pasar: [BPS, pekerja Agustus 2025 (DDTC)](https://news.ddtc.co.id/berita/nasional/1814987/bps-746-juta-orang-menganggur-pada-agustus-2025), [BPS Statistik Telekomunikasi 2024](https://www.bps.go.id/en/publication/2025/08/29/beaa2be400eda6ce6c636ef8/telecommunication-statistics-in-indonesia-2024.html), [StatCounter desktop OS Indonesia](https://gs.statcounter.com/os-market-share/desktop/indonesia).

## 14. Tahapan

| Tahap | Keluaran | Status |
|---|---|---|
| 1. Migrasi ke Tauri (Fase 0–9) | Paritas fitur v1, hardening | ✅ |
| 2. aina Cloud + kredit | Akun, kredit, Midtrans sandbox, web langganan, cloud-only | ✅ (production 20 Okt 2026) |
| 3. Rebrand aina 1.0.0 | Nama, ikon, installer wizard, listing Store | ✅ (3 Nov 2026, belum di-commit) |
| 4. Beta tertutup | 10–20 penguji, QA paritas, perbaikan | Berjalan |
| 5. Rilis publik | Code signing, updater, Midtrans production, pentest, review hukum, Store live | Berikutnya |
| 6. Pertumbuhan | Aplikasi kerja non-Google, telemetry opt-in, Enterprise, harga yang divalidasi | Nanti |

## 15. Asumsi
- Pengguna memakai Windows 10/11 x64 dan terhubung ke internet.
- Pengguna mau masuk dengan akun Google.
- Harga dan ketersediaan model Anthropic, OpenAI, ElevenLabs, dan Composio relatif stabil.
- Claude Code tidak wajib; mesin bawaan aina cukup untuk Sprite.
- Midtrans menyetujui tokenisasi kartu untuk merchant.

## 16. Keputusan yang perlu diambil
1. Harga per kredit Pro (sekarang hampir sama dengan Basic) dan kuota Basic.
2. Apakah Kerjakan dan Sprite perlu harga kredit yang lebih ringan dari tanya layar.
3. Telemetry opt-in atau survei untuk mengukur aktivasi dan retensi.
4. Pembayaran di build Store: tetap Midtrans, atau ikut aturan pembayaran Store.
5. Urutan aplikasi kerja non-Google dan perlu tidaknya verifikasi OAuth sendiri.
