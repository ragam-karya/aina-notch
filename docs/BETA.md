# aina Beta: panduan penguji

Terima kasih sudah ikut mencoba aina! aina adalah teman di laptopmu yang paham layar: tahan tombol, bicara, dan aina menjawab sambil menunjuk, menggambar, atau mengerjakan sesuatu untukmu.

## 1. Pasang
1. Jalankan `aina_<versi>_x64-setup.exe`. Tidak perlu hak admin.
2. Windows mungkin menampilkan **"Windows protected your PC"** karena aplikasi beta belum ditandatangani. Klik **More info → Run anyway**.
3. Setelah terpasang, aina berjalan di tray (pojok kanan bawah), dan Settings terbuka.

## 2. Gratis dan berbayar
**Smart notch gratis, tanpa akun:** jam, cuaca, jadwal sholat, timer dan pomodoro, pengingat, musik yang sedang diputar, catatan, fokus, dan shortcut. Hover notch di atas layar untuk melihatnya.

**AI aina berbayar.** Klik **Masuk dengan Google** di Settings → Akun (atau tombol **Masuk** yang muncul di notch saat kamu bertanya ke aina). Akun baru dapat **10 kredit gratis** sekali per PC untuk mencoba. Setelah habis, pilih langganan atau Top-Up di Settings → Akun. Sisa kredit terlihat di Settings → Akun dan saat notch di-hover.

**Aplikasi kerja** (Gmail, Google Calendar, Drive, Docs, Sheets, Tasks; termasuk kalender Google di notch) khusus akun berbayar: langganan aktif, atau setelah membeli Top-Up. Aplikasi lain (Outlook, Notion, Slack, dan seterusnya) masih ditandai **Segera**.

## 3. Yang bisa dicoba
| Coba ini | Caranya |
|---|---|
| Tanya soal layar | Tahan **Ctrl+Alt**, bicara ("tombol save di mana?"), lepas. Sambil menahan, kamu bisa melingkari atau menggaris bawahi sesuatu dengan mouse. |
| Panduan klik demi klik | "Cara ganti tema di VS Code?" Ikuti titik biru. |
| Kerjakan untukku | "Buka Calculator, hitung 12×3" atau "rapikan folder Unduhan". aina minta izin dulu sebelum mengubah sesuatu. |
| Dikte | Tekan **Ctrl+Win** sekali, bicara, tekan lagi. Hasilnya dirapikan, diringkas kalau panjang, tampil di notch, dan tersimpan di `Documents\aina\Dikte`. |
| Catatan & pengetahuan | Hover notch → **📝 Catatan** / **📚 Pengetahuan**. Seret file (PDF, Word, Markdown) ke notch, lalu tanya isinya. |
| Fokus di folder proyek | Seret folder proyek ke notch. |
| Ngobrol realtime | Settings → Suara → Mesin suara: **realtime**. Tap Ctrl+Alt (sebentar) lalu ngobrol bebas; bilang "udah dulu" untuk selesai. |
| Hey aina | Settings → nyalakan "Hey aina", lalu ucapkan "Hey aina" diikuti pertanyaan. |
| Timer & pengingat | "Timer 10 menit nulis", "ingetin aku jam 3 meeting". |
| aina latar belakang | "Delegasikan: rangkum berita ekonomi hari ini". Lihat hasilnya di tray → aina latar belakang. |
| Aplikasi kerja (berbayar) | Settings → Aplikasi kerja: sambungkan Gmail/Calendar, lalu "cek 3 email terbaru". |

**Esc** selalu menghentikan aina.

## 4. Kira-kira pemakaian kredit
- tanya layar (dengan suara ElevenLabs): ±1 kredit, jadi 10 kredit gratis cukup untuk ±10 pertanyaan
- mode realtime: ±1,8 kredit per jawaban
- dikte: ±Rp 10 per menit
- Kerjakan: ±Rp 1–5 rb per tugas
- aina latar belakang: ±Rp 1–10 rb per tugas

Kalau kredit habis, notch menampilkan "Kredit habis" dengan tombol **Beli kredit**. Notch tetap gratis.

## 5. Kalau ada masalah
- Tray → **Kirim masukan…** membuka formulir masukan.
- Tray → **Buka log** membuka `aina.log`. Lampirkan file itu bersama ceritamu: apa yang kamu lakukan, apa yang terjadi, dan apa yang kamu harapkan. Log hanya berisi error teknis, tanpa isi ucapan atau layarmu.

## 6. Privasi
- aina tidak menyimpan API key apa pun di laptopmu; semuanya lewat server aina Cloud.
- Pertanyaan, suara, dan tangkapan layar diteruskan ke penyedia AI (Anthropic, OpenAI, ElevenLabs) dan **tidak disimpan** di server aina.
- Yang tersimpan di laptopmu: setelan, catatan dikte (di `Documents\aina\Dikte`), dan daftar file pengetahuan (path-nya saja).
- Detail: https://aina.ai.id/api/privasi
