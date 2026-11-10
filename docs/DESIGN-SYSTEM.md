# Design system aina

Satu acuan visual untuk semua yang dilihat user: Settings, jendela Agents, notch (Dynamic Island), overlay, installer, halaman cloud dan landing. Suara, nama dan nada bicara ada di `../BRANDING.md`; dokumen ini mengatur bentuk, warna, huruf dan gerak.

Sumber kebenaran:
- **Logo:** `crates/aina-app/icons/brand/` (dibuat ulang oleh `npm run icon`, `scripts/brand-icon.ps1`).
- **Token UI:** `src/ui/style.css` (`:root`) dan blok `<style>` di `src/ui/overlay.html` (`#island`, `--c-*`) di folder v1, disalin ke `ui-dist/` oleh `npm run sync-ui`.

Nilai di dokumen ini diambil persis dari file-file itu. Warna logo diambil langsung dari piksel `aina-logo.png`.

---

## 1. Logo

| File | Isi | Pakai untuk |
|---|---|---|
| `brand/aina-mark.png` | **Mark saja, tanpa tulisan**, transparan, 1024×1024, margin 8% | ikon app, tray, avatar, tile Store, notch, favicon, tempat kecil atau persegi |
| `brand/aina-logo.png` | Mark + tulisan "aina" biru tua | latar terang |
| `brand/aina-logo-dark.png` | Mark + tulisan "aina" putih | latar gelap (sidebar Settings, installer) |
| `icons/source.png` | Sama dengan `aina-mark.png` | sumber `tauri icon` (semua ukuran ikon dan MSIX) |
| `brand/*-original.png` | Gambar asli di atas latar polos | sumber script saja, jangan dipakai langsung |

Salinan untuk UI ada di `assets/brand/` (v1) → `ui-dist/assets/brand/`.

### Anatomi mark
Dua bentuk yang membentuk huruf "A" miring ke kanan:
- **Panah oranye** di kiri bawah: kursor, "kamu yang menunjuk".
- **Batang biru** membulat di kanan: aina yang menjawab dan menunjuk balik.

Jangan memisahkan kedua bentuk, menukar warnanya, memutar mark, memberi outline, bayangan atau gradasi, atau menggambar ulang mark dengan CSS/SVG. Selalu pakai file PNG di atas. (Belum ada versi vektor/SVG. Kalau dibutuhkan, buat dari file asli, jangan dijiplak.)

### Kapan mark, kapan logo lengkap
- **Mark saja** kalau lebarnya di bawah ~120 px, bentuknya persegi, atau nama "aina" sudah tertulis di dekatnya (judul jendela, notifikasi, notch).
- **Logo lengkap** di layar pembuka, header Settings, installer, halaman cloud dan landing.
- Logo lengkap pada tinggi minimal 24 px; mark minimal 16 px (ukuran tray).

### Ruang kosong
Sisakan ruang kosong di sekitar logo minimal setinggi titik oranye di atas huruf "i" (± 20% tinggi tulisan). Di `aina-mark.png` ruang itu sudah termasuk (margin 8%).

### Latar
- Logo terang di latar `--bg`/`--panel` terang atau putih.
- Logo gelap di latar `#14171c` ke bawah atau hitam notch.
- Mark terbaca di kedua tema. Jangan taruh di atas biru atau oranye brand, atau di atas foto ramai.

---

## 2. Warna

### 2.1 Warna brand (dari logo)

| Token | Hex | Asal | Peran |
|---|---|---|---|
| `brand-blue` | `#0058F5` | batang biru logo | warna aina: aksi utama, link di latar terang |
| `brand-orange` | `#FE6D01` | panah dan titik "i" | "kamu": dikte, menunggu keputusanmu, sorotan hangat |
| `brand-navy` | `#061334` | tulisan "aina" | teks judul brand di latar terang |

`icons/source.png` (mark) memakai biru `#0169F3` dan oranye `#FE8A19`, sedikit lebih terang dari logo lengkap. Untuk token pakai nilai logo lengkap di atas.

Arti warna tetap sama dengan `BRANDING.md`: **biru = aina**, **oranye = kamu sedang bicara atau memutuskan**, **merah = hanya masalah**.

### 2.2 Token UI (Settings, Agents, jendela kecil) — `style.css`

| Token | Terang | Gelap | Pakai |
|---|---|---|---|
| `--bg` | `#f6f7f9` | `#14171c` | latar jendela |
| `--panel` | `#ffffff` | `#1c2027` | kartu, sidebar, field, dialog |
| `--text` | `#151a22` | `#e8ecf2` | teks utama |
| `--muted` | `#5f6b7a` | `#96a1b0` | hint, label sekunder, judul seksi `h2` |
| `--line` | `#e2e6ec` | `#2c323c` | garis, border field/kartu, track switch |
| `--chip` | `#eef3fb` | `#232a35` | chip, hover tombol, `kbd`, baris rutin |
| `--accent` | `#2b8aff` | `#2b8aff` | tombol utama, fokus, switch aktif, item terpilih |
| `--accent-text` | `#fff` | `#fff` | teks di atas `--accent` |
| `--danger` | `#d9363e` | `#d9363e` | tombol/teks bahaya, error |
| `.ok` | `#1f9d55` | `#1f9d55` | status berhasil |

Tema gelap mengikuti Windows (`prefers-color-scheme`), tanpa toggle sendiri.

### 2.3 Token notch (selalu gelap, di atas hitam) — `overlay.html`

| Token | Nilai | Pakai |
|---|---|---|
| latar notch | `#000000` | bentuk island (harus hitam pekat supaya menyatu dengan tepi layar) |
| `--c-text` | `#eef2f8` | teks utama (`.label` `#e9eef6`) |
| `--c-sub` | `#8e9bb0` | ucapanmu yang dikutip, teks sekunder |
| `--c-surface` / `-2` | `rgba(255,255,255,.055)` / `.09` | baris, kartu di dalam notch |
| `--c-track` | `rgba(255,255,255,.14)` | track progress bar |
| `--c-accent` | `#5aa9ff` | caret mengetik, aksen |
| `--c-ok` | `#3ddc97` | selesai |
| `--c-warn` | `#ffb27a` | peringatan |
| `--c-timer` | `#ff8a3d` | timer, dikte |
| `--c-danger` | `#ff5a64` | error, tombol hentikan |
| `--c-music` | `#a78bfa` | media yang sedang diputar |

### 2.4 Kontras (WCAG, dihitung)

| Pasangan | Rasio | Status |
|---|---|---|
| `--muted` di `--bg` terang | 5.06 | lolos |
| `--muted` gelap di `--bg` gelap | 6.86 | lolos |
| `--c-sub` di hitam | 7.47 | lolos |
| putih di `brand-blue` `#0058F5` | 5.65 | lolos |
| `brand-navy` di `brand-orange` | 6.43 | lolos |
| **putih di `--accent` `#2b8aff`** | **3.40** | **gagal untuk teks 14 px** (tombol primer) |
| **putih di `brand-orange`** | **2.84** | **gagal**: teks di atas oranye selalu `brand-navy` |
| **`.ok` `#1f9d55` di putih** | **3.49** | **gagal untuk teks kecil** |
| `brand-blue` di `--bg` gelap | 3.18 | hanya untuk ikon/garis, bukan teks |

Aturan yang mengikuti dari tabel ini:
- Teks putih di atas biru: pakai `brand-blue` `#0058F5`, bukan `#2b8aff`.
- Di tema gelap, aksen untuk teks tetap `#2b8aff` (5.29 di `#14171c`) atau `#5aa9ff`.
- Jangan pernah teks putih di atas oranye.

### 2.5 Selisih dengan logo (belum diputuskan)
`--accent` UI (`#2b8aff`) lebih muda dari biru logo (`#0058F5`), dan oranye UI (`#ff8a3d`) beda dari oranye logo (`#FE6D01`). Usulan, belum diterapkan di kode:

```css
:root {
  --brand-blue: #0058F5; --brand-orange: #FE6D01; --brand-navy: #061334;
  --accent: var(--brand-blue);               /* tema terang: putih di atasnya 5.65:1 */
}
@media (prefers-color-scheme: dark) {
  :root { --accent: #2b8aff; }               /* tema gelap: tetap terbaca di #14171c */
}
```

Notch tetap memakai biru terang (`#5aa9ff`, tombol `#2b8aff`) karena latarnya hitam.

---

## 3. Tipografi

- **Typeface:** Urbanist (variable, berat 100–900, SIL OFL), dibundel di `assets/fonts/Urbanist-Variable.ttf`. Halaman tidak boleh memuat font dari web.
- **Fallback:** `"Segoe UI Variable Text", "Segoe UI", sans-serif`.
- **Mono:** `Consolas, "Cascadia Mono", monospace` (kbd, log, diff).

### Skala jendela (`style.css`, `agents.css`)
| Gaya | Ukuran / tinggi baris | Berat | Pakai |
|---|---|---|---|
| `h1` | 20 px | 700 | judul jendela |
| `body` | 14 px / 1.5 | 400 | teks dasar |
| `label` | 14 px | 600 | label field |
| `h2` seksi | 13 px, uppercase, `letter-spacing .06em`, `--muted` | 700 | judul grup di Settings |
| `chip`, `.hint`, `.status` | 12.5 px | 400 | hint dan chip |
| `.meta` | 11.5 px | 400 | waktu, biaya di bawah pesan |
| `kbd` | 12 px mono | 400 | tombol keyboard (hanya di UI tertulis, tidak diucapkan) |

### Skala notch (`overlay.html`)
| Token | Ukuran | Pakai |
|---|---|---|
| `--fs-display` | 26 px | jam di peek |
| `.say` | 16 px / 1.45 | jawaban aina |
| `--fs-lg` | 14 px | judul panel |
| `--fs-md` / `.label` | 13 px, 600 | status ("Mendengar", "Berpikir…") |
| `--fs-sm` | 12 px | baris panel |
| `--fs-xs` | 11 px | keterangan kecil |

Nama produk selalu **aina** huruf kecil, termasuk di judul dan awal kalimat.

---

## 4. Ruang, radius, bayangan

**Spasi** (kelipatan 4): `--sp-1` 4 · `--sp-2` 8 · `--sp-3` 12 · 16 · 18 (padding kartu) · 20–22 (padding area pesan). Jarak antar elemen dalam baris 6–10 px.

**Radius**
| Nilai | Pakai |
|---|---|
| 5 px | `kbd` |
| 8 px | tombol, field |
| 10 px | `--r-tile`, artifact chip, log, diff |
| 12 px | item daftar, baris rutin |
| 14 px | `--r-card`, kartu |
| 16 px (sudut ekor 4 px) | gelembung chat |
| 18 px | composer, dialog |
| 999 px | `--r-pill`, chip, switch |

**Bayangan**
- `--shadow` terang: `0 1px 2px rgba(16,24,40,.04), 0 2px 8px rgba(16,24,40,.04)`
- `--shadow` gelap: `0 1px 2px rgba(0,0,0,.25), 0 4px 14px rgba(0,0,0,.18)`
- Dialog: `0 24px 60px rgba(0,0,0,.3)`, backdrop `rgba(0,0,0,.35)` + `blur(3px)`
- Notch terbuka: `0 8px 26px rgba(0,0,0,.35)`
- Fokus: cincin `0 0 0 3px` aksen 22–30% (bukan outline keras).

---

## 5. Gerak

| Token | Nilai | Pakai |
|---|---|---|
| `--ease-out` | `cubic-bezier(.2, .85, .25, 1)` | muncul, pindah halaman, pesan masuk |
| `--ease-pop` | `cubic-bezier(.34, 1.5, .6, 1)` | tekan tombol, switch, dialog, sedikit memantul |
| `--t-page` | 220 ms | ganti halaman Settings |
| hover | 180 ms `ease` | warna, border |
| tekan | `scale(.96)` dalam 80 ms (notch `.92`) | umpan balik klik |

- Notch membuka dengan pegas (`step()` di `island.js`), bukan transisi CSS.
- Elemen baru naik 4–10 px sambil memudar; pesan user datang dari kanan, aina dari kiri.
- `prefers-reduced-motion` mematikan semua animasi (sudah di `style.css`).
- Tidak ada yang bergerak saat aina diam (prinsip "hadir, bukan menyela"): loop gambar notch dan mascot tidur.

---

## 6. Komponen

### Tombol
| Varian | Tampilan |
|---|---|
| default | `--panel`, border `--line`, radius 8, padding 8×14; hover `--chip` |
| `.primary` | latar `--accent`, teks putih 600, glow aksen 30% |
| `.danger` | teks `--danger`; hover latar merah 10% |
| disabled | opacity .5 |
| chip (`.chips button`) | pill, `--chip`, 12.5 px, padding 5×12; hover teks aksen |

Satu tombol primer per area. Label tombol kata kerja pendek ("Simpan", "Sambungkan").

### Field
Input, select, textarea: padding 8×10, radius 8, border `--line`; hover border aksen 35%; fokus border `--accent` + cincin 3 px.

### Switch
Checkbox di `.check` digambar sebagai switch 36×20, knob putih 14 px, aktif `--accent`, knob melompat dengan `--ease-pop`.

### Kartu
`--panel`, border `--line`, radius 14, padding 18, `--shadow`; hover border aksen 18%.

### Daftar sidebar (Agents)
Item radius 12, hover `--chip` + geser 2 px; terpilih: latar aksen 13%, bilah aksen 3 px di kiri, nama berwarna aksen.

### Chat
- Pesan user: gradasi aksen, teks putih, radius 16/16/4/16, maks 70% lebar.
- Pesan aina: `--panel`, border `--line`, radius 16/16/16/4.
- Sedang bekerja: tiga titik aksen 6 px yang naik-turun.
- Composer: satu field radius 18 dengan tombol kirim di dalamnya.

### Dialog
Radius 18, lebar 440, `--panel`, masuk dengan `--ease-pop`. Dialog diff selalu gelap (`#12141c`).

### Status
Titik 8 px kuning `#f5a524` berdenyut = tugas berjalan. Teks `.ok`/`.err` 12.5 px.

### Notch (Dynamic Island)
- Hitam pekat, menempel di tepi atas, "telinga" melengkung di kedua sisi (`--ear`).
- Mode: rest → mini (pill + titik) → peek (jam, timer, media) → turn / panel / drop / upload.
- Tombol notch: primer `#2b8aff` (hover `#4a9bff`); tile `rgba(255,255,255,.07)` dengan ikon `#8fb8ff`; ikon 28 px transparan; bahaya merah 18%; play putih bulat 32 px.
- Pertanyaan izin selalu: **Boleh** (primer) · **Boleh semua** · **Jangan**.
- Wajah: mascot Rive (`aina-mascot.riv`) di baris turn, pill dan peek. Ekspresi mengikuti status (alert untuk izin, sleeping setelah 10 menit sepi).

### Overlay
Kursor biru aina (panah dengan glow biru, garis putih tipis), sorotan, bentuk dan tulisan tangan bergaya marker. Ini wajah aina di layar, jangan diganti bentuk geometri kaku.

---

## 7. Ikon
- Ikon garis 24×24 (`src/ui/icons.js`, `ainaIcon(name)`), digambar dengan `currentColor` sehingga mengikuti warna tombol. Beberapa (play, pause) diisi penuh.
- Tidak memakai emoji atau glyph teks (⏮ ▶ ×) sebagai ikon: tampilannya beda per font dan terlalu berat.
- Tombol ikon di notch 28 px (bahaya dan play 32 px).
- Jangan memakai logo produk lain sebagai ikon; nama aplikasi pihak ketiga ditulis sebagai teks.

---

## 8. Yang tidak boleh
- Menggambar ulang atau mengubah warna mark.
- Gradasi biru-ungu sebagai dekorasi (satu-satunya pengecualian: gelembung pesan user).
- Teks putih di atas oranye.
- Memuat font, ikon atau gambar dari CDN.
- Notch berwarna selain hitam.
- Menulis "Aina", "AINA", "B.A.D.I." atau "Badi" di mana pun yang terlihat user.
