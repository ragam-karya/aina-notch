# QA paritas v1 → v2 (Fase 9)

Daftar uji manual untuk beta tertutup. Jalankan v2 dengan profil terpisah supaya data v1 aman:

```powershell
$env:AINA_PROFILE = "D:\aina-beta"; .\aina.exe
```

Setiap baris: ✅ sama dengan v1 · ⚠️ beda, tapi bisa diterima · ❌ bug. Catat juga versi Windows, skala layar, dan jumlah monitor.

## A. Tanya layar (tahan Ctrl+Alt)
| # | Langkah | Yang diharapkan |
|---|---|---|
| A1 | Tahan, tanya "tombol save di mana?", lepas | Notch menampilkan Mendengar → Berpikir → Menjawab. Pointer menunjuk tombol, suara dan teks muncul bersamaan. |
| A2 | Tahan sambil melingkari sesuatu dengan mouse, lepas tanpa bicara | Lingkaran tergambar, lalu aina menjelaskan bagian yang dilingkari. |
| A3 | Garis bawahi teks, lalu tanya | Garis bawah tergambar dan jawabannya sesuai teks itu. |
| A4 | Diam menunjuk 0,6 detik | Muncul cincin tunjuk dan nama elemen di bawah kursor. |
| A5 | Tanya "cara ganti tema di VS Code", ikuti panduan | Panduan klik demi klik: tiap klik di target memunculkan langkah berikutnya. Klik di luar target mengakhiri panduan. |
| A6 | Tekan Esc saat aina bicara | Suara dan visual berhenti seketika. |
| A7 | Tanya lagi dalam 10 menit ("yang tadi maksudnya apa?") | aina ingat percakapan sebelumnya. |
| A8 | Dua monitor dengan skala berbeda (100% + 150%) | Pointer dan tulisan tangan tepat di monitor yang benar. **Belum pernah diuji.** |
| A9 | Tahan Ctrl+Alt tanpa bicara | "Aku belum dengar jelas…" (bukan diam saja). Tap singkat tidak memunculkan apa-apa. |
| A10 | Tanya lalu tunggu jawaban yang lama | Setelah 1,5 dtk "Bentar, aku lihat dulu ya.", setelah 6 dtk timer, setelah 20 dtk petunjuk Esc. |
| A11 | Settings → ubah tombol bicara, simpan | Combo baru langsung berlaku tanpa restart. |
| A12 | Tray → Ketik pertanyaan, kirim | Bar tertutup sebelum screenshot. Klik di luar bar juga menutupnya. |
| A13 | Ctrl+Win (tahan), ucapkan beberapa kalimat | Notch menampilkan "✓ Dikte tersimpan" + isi dikte (dirapikan Haiku kalau "Rapikan hasil dikte" aktif), tombol Salin / Buka file / Tutup. File `Documents\aina\Dikte\<tanggal jam>.md` ada. Tidak ada yang terketik ke aplikasi aktif. |
| A14 | Dikte panjang (>120 kata, ±1 menit bicara) | Notch dan file punya bagian Ringkasan di atas isi. Isi yang panjang bisa di-scroll di dalam notch. |

## B. Suara
| # | Langkah | Yang diharapkan |
|---|---|---|
| B1 | TTS ElevenLabs / OpenAI / Windows (Settings → Suara) | Ketiganya terdengar. Kalau cloud lambat, aina jatuh ke suara berikutnya tanpa macet. |
| B2 | Mic dicabut saat tahan bicara | Muncul pesan jelas, tidak crash. |
| B3 | Settings → "Hey aina" nyala, ucapkan "Hey aina" lalu pertanyaan | aina menjawab singkat ("Ya, kenapa?"), mendengar, dan berhenti sendiri saat kamu diam. Tidak terpicu saat video/lagu diputar. |
| B4 | Settings → Mesin suara: realtime (login aina Cloud) | Tahan = tanya sekali. Tap = sesi percakapan, bisa dipotong dengan bicara, berakhir saat bilang "udah dulu" atau diam 30 dtk. Pertanyaan soal layar tetap menunjuk/menggambar. Persentase pemakaian di Settings naik setelah sesi. |
| B5 | Bunyi | Chime saat mulai tahan, saat pertanyaan terkirim, tiap langkah panduan, dan saat agent mulai/selesai. |
| B6 | Settings → Jalan saat Windows mulai, lalu restart Windows | aina berjalan sendiri. |

## C. Notch
| # | Langkah | Yang diharapkan |
|---|---|---|
| C1 | "Timer 5 menit nulis" | Pill timer tampil, hitung mundur, berbunyi dan bicara saat habis. |
| C2 | "Ingatkan jam 3 sore minum" | Pengingat tersimpan dan bertahan setelah restart. |
| C3 | Hover notch | Peek: jam, timer, media, fokus, dan pintasan. |
| C4 | "Fokus ke folder ini" | aina bilang folder bisa di-drag ke notch. Zona dan animasinya baru muncul saat folder diseret. Setelah di-drop muncul notice "📁 Fokus · nama", dan pill notch menunjukkan fokus. |
| C5 | "Upload file ini ke Drive", lalu drop file | Animasi awan dan progress, lalu notice berisi link Drive. |
| C6 | Putar lagu di Spotify, lalu hover | Judul lagu tampil. Tombol play/next berfungsi. |
| C7 | Seret folder proyek (ada .git/package.json) dari Explorer ke notch, tanpa diminta | Notch membuka "Taruh di sini". Setelah dilepas: fokus ke folder itu, notice punya tombol "Jadikan pengetahuan". |
| C8 | Seret file PDF/DOCX/MD atau folder dokumen ke notch | Notice "📚 Masuk ke pengetahuanku" + "Lihat semua". Tanya isinya ("cuti tahunan berapa hari?"): aina menjawab dari file itu dan menyebut nama file-nya. |
| C9 | Hover notch → 📝 Catatan / 📚 Pengetahuan | Daftar di dalam notch, scroll kalau panjang. Klik catatan: isinya tampil (← Catatan untuk kembali). × di pengetahuan mengeluarkannya (file-nya tetap ada). |
| C10 | Seret sesuatu di dekat atas layar lalu lepas di tempat lain | Notch tidak menyisakan zona apa pun; klik di area atas layar tetap normal. |

## D. Kerjakan untukku
| # | Langkah | Yang diharapkan |
|---|---|---|
| D1 | Pertama kali "kerjakan" | Dialog izin kontrol komputer muncul sekali. |
| D2 | "Buka Calculator, hitung 12×3" | Selesai lewat UIA dan hasilnya diucapkan. |
| D3 | "Hapus file x.txt di Documents/aina" | aina bertanya "Aku mau hapus … ke Recycle Bin. Boleh?". Jawab "jangan", dan file tetap ada. |
| D3b | Sama, tapi jawab "hmm, apa maksudnya?" | **Tidak** dianggap setuju: file tetap ada. Hanya jawaban jelas ("boleh", "iya", "oke") yang menyetujui. |
| D4 | Esc di tengah tugas | Tugas berhenti, aina bilang "Oke, aku berhenti." |
| D5 | Ekstensi browser dipasangkan (kode 6 digit) | Tugas di web memakai tool browser (read_page, find). |

## E. aina latar belakang
| # | Langkah | Yang diharapkan |
|---|---|---|
| E1 | "Delegasikan: rangkum berita ekonomi hari ini" | Agent General jalan dan pill notch menampilkan baris log. Setelah selesai muncul notice + notifikasi Windows, dan laporan ada di output/. |
| E2 | Buka jendela Agents | Agent lama dari v1 tampil, chat dan log terbaca. |
| E3 | Minta "tiap 60 menit cek …" | Routine tercatat dan jalan sendiri, juga setelah PC bangun dari sleep. |
| E4 | Mode fokus + "review kode" | Laporan ada di aina-reports/ dan tidak ada kode yang berubah. |

## F. Aplikasi kerja
| # | Langkah | Yang diharapkan |
|---|---|---|
| F1 | Settings → Aplikasi kerja: nyalakan Gmail | Halaman login terbuka. Setelah login, status berubah jadi tersambung. |
| F2 | "Cek 3 email terbaru" | Pengirim dan subjek dibacakan lewat Composio (tidak membuka browser). |
| F3 | "Kirim email ke diriku: tes" | aina bertanya dulu. Setelah dijawab "boleh", email terkirim. |

## G. aina Cloud
| # | Langkah | Yang diharapkan |
|---|---|---|
| G1 | Settings → Akun → Masuk dengan Google | Browser login. Setelah berhasil, aina terbuka, muncul notice "Coba gratis 7 hari sudah aktif", dan Settings menampilkan akun. |
| G2 | Tanya layar (app tidak menyimpan API key apa pun) | Berjalan lewat Cloud. Sisa kredit di Settings → Akun dan peek notch berkurang. |
| G3 | Pilih paket Plus → bayar (Midtrans sandbox) | Status paket berubah jadi Plus dalam ±15 detik setelah lunas. |
| G4 | Habiskan jatah sesi (tes dengan akun Lite) | aina bilang jatahnya habis dan kapan kembali (jam WIB). Tugas yang sedang jalan tetap selesai. |
| G5 | Keluar, lalu tahan Ctrl+Alt | Notch: "Masuk ke aina Cloud dulu ya", Settings terbuka di bagian Akun. Tidak ada kolom API key di Settings. |
| G7 | `cmdkey /list \| findstr aina` | Hanya `ainaCloudToken` dan `extensionToken`; key penyedia lama dari build sebelumnya terhapus saat start. |
| G6 | Hapus akun | Akun hilang. Login lagi memulai akun baru. |

## H. Lain-lain
| # | Langkah | Yang diharapkan |
|---|---|---|
| H1 | Jalankan v2 kedua kalinya | Settings terbuka di instance yang sudah jalan. |
| H2 | Matikan internet, lalu tanya | Pesan "belum bisa dihubungi" yang jelas, tanpa crash. |
| H3 | Rollback: jalankan v1 setelah memakai v2 | v1 tetap membaca key-nya (config v1 tidak diubah). |
