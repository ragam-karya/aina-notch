// Badi's language. 'id' and 'en' are fixed languages; 'auto' (default) answers in whatever language the
// user speaks, and then the loading states, the filler and app messages follow that language too.
// In aina-notch this module only feeds ui-dist/i18n.json (scripts/sync-ui.mjs); the language setting
// lives in Rust, so there is no config module here and lang() falls back to Indonesian.
const config = { load: () => ({}) };

const LANGS = {
  id: { name: 'Indonesian (Bahasa Indonesia)', stt: 'id', voiceLang: 'id' },
  en: { name: 'English', stt: 'en', voiceLang: 'en' },
  auto: { name: null, stt: undefined, voiceLang: null },
};

const STRINGS = {
  id: {
    tooShort: 'Rekamannya kependekan. Coba ngomong sedikit lebih lama ya.',
    markOnly: 'Jelasin yang aku tandai ini.',
    escHint: 'Tekan Esc untuk batal',
    tookTooLong: 'Kelamaan nih, aku batalin dulu. Coba tanya lagi ya.',
    notHeard: 'Aku belum dengar jelas. Tahan tombolnya lalu coba lagi.',
    needOpenAIVoice: 'Isi OpenAI API key di Settings untuk bicara dengan aina, atau pakai "Ketik pertanyaan" dari tray.',
    needOpenAIDictation: 'Isi OpenAI API key di Settings untuk memakai dikte.',
    dictationTooShort: 'Diktenya kependekan. Tahan tombolnya selama kamu ngomong ya.',
    dictationFailed: (m) => `Diktenya gagal: ${m}`,
    mic: (m) => `Mikrofon: ${m}`,
    badAudio: 'Suaranya nggak kerekam jelas. Coba tahan tombolnya sedikit lebih lama ya.',
    badKey: 'API key-nya nggak valid. Cek lagi di Settings ya.',
    rateLimit: 'Lagi kebanyakan permintaan nih. Coba lagi sebentar ya.',
    offline: 'Internetnya lagi nggak nyambung.',
    agentDone: (name) => `${name} selesai`,
    taskDone: 'Beres.',
    consentTitle: 'Izinkan aina mengontrol komputermu?',
    consentBody:
      'aina akan melihat layarmu dan memakai mouse serta keyboard untuk mengerjakan permintaanmu, misalnya membuka aplikasi, mengklik, mengetik, dan mencari di web.\n\n' +
      '• aina selalu minta izin dulu sebelum mengirim, membeli, menghapus, atau mengisi formulir.\n' +
      '• aina tidak akan mengetik kata sandi atau data kartu.\n' +
      '• Tekan Esc kapan saja untuk menghentikannya.\n\n' +
      'Isi layar dikirim ke Claude (Anthropic) selama tugas berjalan. Fitur ini bisa dimatikan lagi di Settings.',
    consentAllow: 'Izinkan',
    consentCancel: 'Jangan sekarang',
    agentAskWrite: (name, file) => `${name} mau bikin file ${file}. Boleh?`,
    agentAskEdit: (name, file) => `${name} mau ngedit ${file}. Boleh?`,
    agentAskCommand: (name, acts, place) => `${name} mau ${acts} di ${place}. Boleh?`,
    agentAct_delete: 'menghapus file ke Recycle Bin',
    agentAct_move: 'memindahkan file',
    agentAct_rename: 'mengganti nama file',
    agentAct_copy: 'menyalin file',
    agentAct_create: 'membuat folder',
    agentAct_write: 'menulis file',
    and: 'dan',
    yourFolders: 'folder kamu',
    agentConfirmHint: 'Jawab "boleh", "boleh semua" atau "jangan" · Enter = ya · Esc = tidak',
    fileWhoMe: 'Aku mau',
    fileWhoAgent: (name) => `${name} mau`,
    fileCount: (n) => `${n} item`,
    fileAsk_delete: (who, what, place) => `${who} hapus ${what}${place ? ` dari folder ${place}` : ''} ke Recycle Bin. Boleh?`,
    fileAsk_move: (who, what, place) => `${who} pindahin ${what} ke folder ${place}. Boleh?`,
    fileAsk_copy: (who, what, place) => `${who} salin ${what} ke folder ${place}. Boleh?`,
    fileAsk_rename: (who, what, name) => `${who} ganti nama ${what} jadi ${name}. Boleh?`,
    fileAsk_create: (who, what, place) => `${who} bikin folder ${what}${place ? ` di ${place}` : ''}. Boleh?`,
    fileAsk_edit: (who, what) => `${who} ngedit ${what}. Boleh?`,
    fileAsk_write: (who, what, place) => `${who} bikin file ${what}${place ? ` di ${place}` : ''}. Boleh?`,
    fileAsk_overwrite: (who, what) => `${who} nulis ulang ${what}. Boleh?`,
    focusOn: (name, where) => `Oke, aku fokus di folder ${name}${where ? ` yang di ${where}` : ''}.`,
    focusOff: 'Oke, fokusnya aku lepas.',
    focusNotFound: (name) => `Aku nggak nemu folder ${name}. Sebutin folder di atasnya, misalnya "${name} di Documents".`,
    focusMany: (n, name, list) => `Ada ${n} folder ${name}: ${list}. Yang mana?`,
    focusIn: (where) => `di ${where}`,
    appAsk: (who, what, app) => `${who} ${what}${app ? ` di ${app}` : ''}. Boleh?`,
    appAskMany: (who, n, apps) => `${who} jalankan ${n} aksi di ${apps}. Boleh?`,
    appVerb_SEND: 'kirim', appVerb_REPLY: 'balas', appVerb_FORWARD: 'teruskan', appVerb_CREATE: 'bikin', appVerb_ADD: 'tambah',
    appVerb_DELETE: 'hapus', appVerb_REMOVE: 'hapus', appVerb_TRASH: 'buang', appVerb_ARCHIVE: 'arsipkan', appVerb_UPDATE: 'ubah',
    appVerb_EDIT: 'edit', appVerb_MOVE: 'pindahin', appVerb_SHARE: 'bagikan', appVerb_INVITE: 'undang', appVerb_POST: 'posting',
    appVerb_SCHEDULE: 'jadwalkan', appVerb_CANCEL: 'batalkan', appVerb_COMMENT: 'komentar di', appVerb_RUN: 'jalankan', appVerb_UPLOAD: 'upload',
    appTo: (target) => `(${target})`,
    appConnecting: (app) => `Aku buka halaman login ${app} di browser. Selesaikan login di sana ya.`,
    appConnected: (app) => `${app} sudah tersambung.`,
    controlOff: 'Oke, nggak jadi. Kalau mau, bisa dinyalakan di Settings kapan aja.',
    brainStopped: 'Oke, aku berhenti.',
    brainLimit: 'Tugasnya terlalu panjang, jadi aku berhenti dulu. Coba pecah jadi langkah yang lebih kecil.',
    brainRefused: 'Maaf, yang itu nggak bisa aku kerjain.',
    brainFailed: (m) => `Aku nggak berhasil ngerjainnya: ${m}`,
    brainDone: 'Beres!',
    brainUnavailable: (m) => `Kontrol komputer belum bisa dipakai: ${m}`,
    confirmHint: 'Enter = ya · Esc = tidak · atau tahan hotkey dan jawab',
    askHint: 'Tahan tombol bicara, lalu jawab',
    declined: 'Oke, nggak jadi.',
    notchAllow: 'Boleh',
    notchAllowAll: 'Boleh semua',
    notchDeny: 'Jangan',
    notchTypeAnswer: 'Ketik jawaban',
    notchCancel: 'Batal',
    notchOpen: 'Buka',
    dropFolderTitle: 'Taruh folder-nya di sini',
    dropFolderHint: 'atau sebut namanya, nanti aku cariin',
    dropFilesTitle: 'Taruh file-nya di sini',
    dropFilesHint: 'Bisa beberapa file sekaligus, maksimal 100 MB per file',
    dropNeedFolder: 'Itu file, bukan folder. Taruh folder-nya ya.',
    dropOneFolder: 'Satu folder aja ya.',
    dropNeedFiles: 'Itu folder. Buat upload, taruh file-nya ya.',
    dropEmpty: 'Aku nggak nemu file di situ. Coba taruh lagi ya.',
    dropTooBig: (name) => `${name} lebih dari 100 MB, jadi belum bisa aku upload.`,
    dropSkippedFolders: (n) => `${n} folder aku lewati, yang aku upload cuma file-nya.`,
    uploading: (name, i, n) => `Mengunggah ${name}${n > 1 ? ` (${i}/${n})` : ''}…`,
    uploaded: (n) => (n === 1 ? 'Sudah aku upload ke Google Drive.' : `${n} file sudah aku upload ke Google Drive.`),
    uploadFailed: (m) => `Upload-nya gagal: ${m}`,
    driveNeedsProjectKey: 'Upload file butuh Composio project key (ak_…). Ganti key-nya di Settings ya.',
    driveNoComposio: 'Isi Composio API key di Settings dulu, nanti aku bisa upload ke Google Drive.',
    driveNotLinked: 'Google Drive belum tersambung. Aku buka halaman login-nya dulu ya, habis itu taruh lagi file-nya.',
    timerDone: (label) => (label ? `Waktunya habis: ${label}.` : 'Waktunya habis.'),
    pomodoroBreak: (min) => `Satu sesi fokus selesai. Istirahat ${min} menit dulu ya.`,
    pomodoroWork: 'Istirahatnya selesai. Lanjut fokus lagi yuk.',
    timerStopped: 'Timer-nya aku matikan.',
    reminderDue: (text) => `Pengingat: ${text}.`,
    reminderLate: (text) => `Tadi ada pengingat yang kelewat: ${text}.`,
    agentsStopped: 'Semua Sprite aku hentikan.',
    spritesFixed: 'Sprite sudah tetap: Ciro, Mino, Routine, Filo, Nilo dan Daro. Kasih tugas ke salah satunya.',
    spriteViewChanges: 'Lihat perubahan',
    taskSeeError: 'Lihat error',
    calSyncFailed_auth: 'Akses Google Calendar perlu disambung ulang',
    calSyncFailed_scope: 'Izin Google Calendar kurang, sambungkan ulang',
    calSyncFailed_rate: 'Google Calendar sedang membatasi permintaan',
    calSyncFailed_offline: 'Tidak tersambung ke internet',
    calSyncFailed_error: 'Perubahan ke Google Calendar gagal',
    island: {
      listening: 'Mendengar…', dictation: 'Mendikte…', transcribing: 'Oke…', thinking: 'Berpikir…', error: 'Ups',
      acting: 'Mengerjakan…', confirm: 'Butuh izinmu', asking: 'Pertanyaan untukmu', conversing: 'Ngobrol…', speaking: 'Menjawab…', looking: 'Melihat area…', seconds: 'dtk',
      spriteTask: 'Tugas', spriteSteps: 'Aktivitas', spriteStatus: 'Status', spriteElapsed: 'Berjalan', spriteOpen: 'Buka detail', spriteStop: 'Hentikan', spriteClose: 'Tutup', spriteAllow: 'Boleh', spriteAllowAll: 'Boleh semua', spriteDeny: 'Jangan',
      spriteChanges: 'Perubahan', spriteBack: 'Kembali', spriteOpenEditor: 'Buka di editor', spriteDiffTooLarge: 'Diff terlalu besar untuk ditampilkan', spriteDiffGone: 'Diff-nya sudah tidak tersimpan (hanya 1 jam).', spriteMore: 'lagi', spriteNewFile: 'file baru', credits: 'kredit',
      stop: 'Stop', stopAgents: 'Hentikan semua', agents: 'Sprite bekerja', focus: 'Fokus', endFocus: 'Selesai fokus', pomodoroWork: 'Pomodoro · fokus', pomodoroBreak: 'Pomodoro · istirahat', timer: 'Timer', reminders: 'Pengingat', nowPlaying: 'Sedang diputar', idle: 'aina siap', locale: 'id-ID',
      pomoFocus: 'Fokus', pomoShort: 'Istirahat pendek', pomoLong: 'Istirahat panjang', pomoSession: 'Sesi {n} dari {m}', pomoPaused: 'Dijeda', pomoReady: 'Siap',
      pomoCycleEnds: 'siklus selesai {t}', pomoToday: '{n} sesi hari ini', pomoStart: 'Mulai',
      musicNone: 'Tidak ada musik diputar', musicNoneHint: 'Putar lagu di Spotify, YouTube, atau pemutar lain',
      focusStart: 'Mulai fokus', focusStartHint: 'Pilih folder project atau buat konteks kerja', focusActive: 'Fokus aktif', focusPick: 'Pilih folder', focusNew: 'Konteks baru',
      focusNote: 'Catatan', focusDone: 'Selesai', focusOpen: 'Buka folder', focusLocked: 'Edit AI hanya di folder ini', focusUnlocked: 'Edit AI tidak dikunci', durH: 'j', durM: 'mnt', durS: 'dtk',
      tasks: 'Tugas', taskActive: 'Aktif', taskHistory: 'Riwayat', taskNeeds: 'Butuh kamu', taskQueued: 'Antre', taskRunning: 'Berjalan', taskFailed: 'Gagal', taskCompleted: 'Selesai', taskCancelled: 'Dibatalkan',
      taskSeeError: 'Lihat error', taskOpen: 'Buka di aina', taskRetry: 'Ulangi', taskStop: 'Hentikan', taskCancel: 'Batalkan', taskKeep: 'Lanjut', taskStopAsk: 'Hentikan {name}?', taskCancelAsk: 'Batalkan {name}?',
      taskMore: '+{n} lainnya', taskSeeAll: 'Lihat semua', taskNoHistory: 'Belum ada tugas yang selesai', taskNoneHere: 'Tidak ada tugas di fokus ini', taskAllProjects: 'Semua', taskRoutine: 'Rutinitas',
      remAgenda: 'Agenda', remTimeline: 'Timeline', remCalendar: 'Kalender', remToday: 'Hari ini', remTomorrow: 'Besok', remOverdue: 'Terlewat', remOngoing: 'Berlangsung', remDone: 'Selesai', remAllDay: 'Seharian', remUntitled: '(tanpa judul)',
      remNone: 'Tidak ada agenda', remNoneAhead: 'Belum ada yang akan datang', remAdd: 'Tambah', remLocal: 'Lokal', remGoogle: 'Google Calendar', remMore: '+{n} lagi', remNow: 'sekarang',
      remInMin: '{n} menit lagi', remInHour: '{n} jam lagi', remInHM: '{h} jam {m} menit lagi', remInDays: '{n} hari lagi', remAgo: '{n} menit lalu', remAgoH: '{n} jam lalu', remSnoozed: 'ditunda sampai {t}',
      remSnooze: 'Tunda', remDelete: 'Hapus', remCancel: 'Batal', remEdit: 'Ubah', remOpen: 'Buka di Google', remDoneBtn: 'Tandai selesai', remUndo: 'Batalkan selesai',
      remReadOnly: 'Kalender ini hanya bisa dibaca', remReadOnlyEvent: 'Acara ini tidak bisa diubah', remSaving: 'Menyimpan ke Google…', remSyncing: 'Sinkron…', remSynced: 'Google', remSyncFail: 'Google gagal sinkron', remLastSync: 'Terakhir sinkron',
      remSyncAuth: 'Akses Google perlu disambung ulang', remSyncScope: 'Izin Google kurang', remSyncRate: 'Google sibuk, coba lagi nanti', remSyncOffline: 'Offline · pakai data terakhir', remRetry: 'Coba lagi', remConnect: 'Sambungkan Google', remReconnect: 'Sambungkan ulang',
      remCompact: 'Ringkas', remComfortable: 'Lega', remPrayerLoading: 'Mengambil jadwal salat…', remPrayerError: 'Jadwal salat belum bisa diambil', remPrayerPast: 'Sudah lewat', remPrayerToday: 'Jadwal salat hari ini', remPrayerSuggest: 'Tampilkan jadwal salat untuk {city}', remPrayerOn: 'Nyalakan',
      homeNoNext: 'Tidak ada agenda', homeAddHint: 'Tambah pengingat di halaman Pengingat', homeTasks: '{n} tugas berjalan', homeNoTasks: 'Tidak ada tugas', homeTasksHint: 'Tugas latar belakang tampil di sini', homeTimerRun: 'Berjalan', homeFocusHint: 'Pilih folder atau konteks kerja', homeMusicHint: 'Putar lagu, kontrolnya ada di sini',
      wxFeels: 'terasa {t}°', wxSetup: 'Masukkan kota',remPrayerGuide: 'Atur kota supaya jadwal salat akurat', remPrayerSetupBtn: 'Atur', remPrayerHide: 'Sembunyikan',
      remNotifyBefore: 'Notifikasi {n} menit sebelum', remNotifyAt: 'Notifikasi tepat waktu', remNotifyOff: 'Notifikasi mati',
      prayer_imsak: 'Imsak', prayer_subuh: 'Subuh', prayer_terbit: 'Terbit', prayer_dhuha: 'Dhuha', prayer_dzuhur: 'Dzuhur', prayer_ashar: 'Ashar', prayer_maghrib: 'Maghrib', prayer_isya: 'Isya',
    },
  },
  en: {
    tooShort: 'That was a bit short. Try talking a little longer.',
    markOnly: 'Explain what I marked.',
    escHint: 'Press Esc to cancel',
    tookTooLong: 'That took too long, so I stopped. Try asking again.',
    notHeard: "I didn't catch that. Hold the keys and try again.",
    needOpenAIVoice: 'Add an OpenAI API key in Settings to talk to aina, or use "Ketik pertanyaan" from the tray.',
    needOpenAIDictation: 'Add an OpenAI API key in Settings to use dictation.',
    dictationTooShort: 'That was too short to dictate. Hold the keys while you talk.',
    dictationFailed: (m) => `Dictation failed: ${m}`,
    mic: (m) => `Microphone: ${m}`,
    badAudio: "Your voice didn't come through clearly. Try holding the keys a bit longer.",
    badKey: "That API key isn't valid. Check it in Settings.",
    rateLimit: 'Too many requests right now. Try again in a moment.',
    offline: "I can't reach the internet right now.",
    agentDone: (name) => `${name} is done`,
    taskDone: 'Done.',
    consentTitle: 'Let aina control your computer?',
    consentBody:
      'aina will look at your screen and use the mouse and keyboard to do what you ask, such as opening apps, clicking, typing and searching the web.\n\n' +
      '• aina always asks first before sending, buying, deleting or submitting forms.\n' +
      '• aina never types passwords or card details.\n' +
      '• Press Esc at any time to stop it.\n\n' +
      'Your screen is sent to Claude (Anthropic) while a task runs. You can turn this off again in Settings.',
    consentAllow: 'Allow',
    consentCancel: 'Not now',
    agentAskWrite: (name, file) => `${name} wants to create ${file}. Okay?`,
    agentAskEdit: (name, file) => `${name} wants to edit ${file}. Okay?`,
    agentAskCommand: (name, acts, place) => `${name} wants to ${acts} in ${place}. Okay?`,
    agentAct_delete: 'delete files to the Recycle Bin',
    agentAct_move: 'move files',
    agentAct_rename: 'rename files',
    agentAct_copy: 'copy files',
    agentAct_create: 'create folders',
    agentAct_write: 'write files',
    and: 'and',
    yourFolders: 'your folders',
    agentConfirmHint: 'Say "yes", "yes to all" or "no" · Enter = yes · Esc = no',
    fileWhoMe: 'I want to',
    fileWhoAgent: (name) => `${name} wants to`,
    fileCount: (n) => `${n} items`,
    fileAsk_delete: (who, what, place) => `${who} delete ${what}${place ? ` from ${place}` : ''} to the Recycle Bin. Okay?`,
    fileAsk_move: (who, what, place) => `${who} move ${what} to ${place}. Okay?`,
    fileAsk_copy: (who, what, place) => `${who} copy ${what} to ${place}. Okay?`,
    fileAsk_rename: (who, what, name) => `${who} rename ${what} to ${name}. Okay?`,
    fileAsk_create: (who, what, place) => `${who} create the folder ${what}${place ? ` in ${place}` : ''}. Okay?`,
    fileAsk_edit: (who, what) => `${who} edit ${what}. Okay?`,
    fileAsk_write: (who, what, place) => `${who} create ${what}${place ? ` in ${place}` : ''}. Okay?`,
    fileAsk_overwrite: (who, what) => `${who} rewrite ${what}. Okay?`,
    focusOn: (name, where) => `Okay, I'm focused on ${name}${where ? ` in ${where}` : ''}.`,
    focusOff: "Okay, I've let go of that folder.",
    focusNotFound: (name) => `I couldn't find a folder called ${name}. Tell me the folder above it, like "${name} in Documents".`,
    focusMany: (n, name, list) => `There are ${n} folders called ${name}: ${list}. Which one?`,
    focusIn: (where) => `in ${where}`,
    appAsk: (who, what, app) => `${who} ${what}${app ? ` in ${app}` : ''}. Okay?`,
    appAskMany: (who, n, apps) => `${who} run ${n} actions in ${apps}. Okay?`,
    appVerb_SEND: 'send', appVerb_REPLY: 'reply to', appVerb_FORWARD: 'forward', appVerb_CREATE: 'create', appVerb_ADD: 'add',
    appVerb_DELETE: 'delete', appVerb_REMOVE: 'remove', appVerb_TRASH: 'trash', appVerb_ARCHIVE: 'archive', appVerb_UPDATE: 'update',
    appVerb_EDIT: 'edit', appVerb_MOVE: 'move', appVerb_SHARE: 'share', appVerb_INVITE: 'invite', appVerb_POST: 'post',
    appVerb_SCHEDULE: 'schedule', appVerb_CANCEL: 'cancel', appVerb_COMMENT: 'comment on', appVerb_RUN: 'run', appVerb_UPLOAD: 'upload',
    appTo: (target) => `(${target})`,
    appConnecting: (app) => `I've opened the ${app} sign-in page in your browser. Finish signing in there.`,
    appConnected: (app) => `${app} is connected.`,
    controlOff: 'Okay, skipping that. You can turn it on in Settings anytime.',
    brainStopped: 'Okay, stopping.',
    brainLimit: 'That task ran too long, so I stopped. Try breaking it into smaller steps.',
    brainRefused: "Sorry, I can't do that one.",
    brainFailed: (m) => `I couldn't finish it: ${m}`,
    brainDone: 'Done!',
    brainUnavailable: (m) => `Computer control isn't available: ${m}`,
    confirmHint: 'Enter = yes · Esc = no · or hold the hotkey and answer',
    askHint: 'Hold the talk keys, then answer',
    declined: 'Okay, never mind.',
    notchAllow: 'Allow',
    notchAllowAll: 'Allow all',
    notchDeny: "Don't",
    notchTypeAnswer: 'Type an answer',
    notchCancel: 'Cancel',
    notchOpen: 'Open',
    dropFolderTitle: 'Drop the folder here',
    dropFolderHint: "or say its name and I'll find it",
    dropFilesTitle: 'Drop the files here',
    dropFilesHint: 'Several files at once is fine, up to 100 MB each',
    dropNeedFolder: "That's a file, not a folder. Drop the folder instead.",
    dropOneFolder: 'Just one folder, please.',
    dropNeedFiles: "That's a folder. To upload, drop the files instead.",
    dropEmpty: "I couldn't find any files there. Try dropping them again.",
    dropTooBig: (name) => `${name} is over 100 MB, so I can't upload it yet.`,
    dropSkippedFolders: (n) => `I skipped ${n} folder${n > 1 ? 's' : ''} and uploaded only the files.`,
    uploading: (name, i, n) => `Uploading ${name}${n > 1 ? ` (${i}/${n})` : ''}…`,
    uploaded: (n) => (n === 1 ? "It's uploaded to Google Drive." : `${n} files are uploaded to Google Drive.`),
    uploadFailed: (m) => `The upload failed: ${m}`,
    driveNeedsProjectKey: 'Uploading files needs a Composio project key (ak_…). Change the key in Settings.',
    driveNoComposio: 'Add a Composio API key in Settings first, then I can upload to Google Drive.',
    driveNotLinked: "Google Drive isn't linked yet. I'm opening its sign-in page; drop the files again after that.",
    timerDone: (label) => (label ? `Time's up: ${label}.` : "Time's up."),
    pomodoroBreak: (min) => `That's one focus session done. Take a ${min}-minute break.`,
    pomodoroWork: "Break's over. Let's get back to it.",
    timerStopped: "I've turned the timer off.",
    reminderDue: (text) => `Reminder: ${text}.`,
    reminderLate: (text) => `You missed a reminder earlier: ${text}.`,
    agentsStopped: "I've stopped all the Sprites.",
    spritesFixed: 'The Sprites are a fixed set: Ciro, Mino, Routine, Filo, Nilo and Daro. Give one of them the task.',
    spriteViewChanges: 'View changes',
    taskSeeError: 'See error',
    calSyncFailed_auth: 'Google Calendar needs to be connected again',
    calSyncFailed_scope: 'aina needs more Google Calendar access; connect it again',
    calSyncFailed_rate: 'Google Calendar is limiting requests right now',
    calSyncFailed_offline: 'Not connected to the internet',
    calSyncFailed_error: "The change to Google Calendar didn't go through",
    island: {
      listening: 'Listening…', dictation: 'Dictating…', transcribing: 'Got it…', thinking: 'Thinking…', error: 'Oops',
      acting: 'Working on it…', confirm: 'Needs your OK', asking: 'Question for you', conversing: 'Talking…', speaking: 'Answering…', looking: 'Looking…', seconds: 's',
      spriteTask: 'Task', spriteSteps: 'Activity', spriteStatus: 'Status', spriteElapsed: 'Running', spriteOpen: 'Open details', spriteStop: 'Stop', spriteClose: 'Close', spriteAllow: 'Allow', spriteAllowAll: 'Allow all', spriteDeny: "Don't",
      spriteChanges: 'Changes', spriteBack: 'Back', spriteOpenEditor: 'Open in editor', spriteDiffTooLarge: 'This diff is too large to show', spriteDiffGone: 'This diff is no longer kept (only for an hour).', spriteMore: 'more', spriteNewFile: 'new file', credits: 'credits',
      stop: 'Stop', stopAgents: 'Stop all', agents: 'Sprites working', focus: 'Focus', endFocus: 'End focus', pomodoroWork: 'Pomodoro · focus', pomodoroBreak: 'Pomodoro · break', timer: 'Timer', reminders: 'Reminders', nowPlaying: 'Now playing', idle: 'aina is ready', locale: 'en-US',
      pomoFocus: 'Focus', pomoShort: 'Short break', pomoLong: 'Long break', pomoSession: 'Session {n} of {m}', pomoPaused: 'Paused', pomoReady: 'Ready',
      pomoCycleEnds: 'cycle ends {t}', pomoToday: '{n} sessions today', pomoStart: 'Start',
      musicNone: 'Nothing playing', musicNoneHint: 'Play something in Spotify, YouTube or another player',
      focusStart: 'Start focus', focusStartHint: 'Pick a project folder or make a work context', focusActive: 'Focusing', focusPick: 'Pick folder', focusNew: 'New context',
      focusNote: 'Notes', focusDone: 'Done', focusOpen: 'Open folder', focusLocked: 'AI edits only in this folder', focusUnlocked: 'AI edits not locked', durH: 'h', durM: 'min', durS: 's',
      tasks: 'Tasks', taskActive: 'Active', taskHistory: 'History', taskNeeds: 'Needs you', taskQueued: 'Queued', taskRunning: 'Running', taskFailed: 'Failed', taskCompleted: 'Completed', taskCancelled: 'Cancelled',
      taskSeeError: 'See error', taskOpen: 'Open in aina', taskRetry: 'Retry', taskStop: 'Stop', taskCancel: 'Cancel', taskKeep: 'Keep going', taskStopAsk: 'Stop {name}?', taskCancelAsk: 'Cancel {name}?',
      taskMore: '+{n} more', taskSeeAll: 'See all', taskNoHistory: 'No finished tasks yet', taskNoneHere: 'No tasks in this focus', taskAllProjects: 'All', taskRoutine: 'Routine',
      remAgenda: 'Agenda', remTimeline: 'Timeline', remCalendar: 'Calendar', remToday: 'Today', remTomorrow: 'Tomorrow', remOverdue: 'Overdue', remOngoing: 'Happening now', remDone: 'Done', remAllDay: 'All day', remUntitled: '(no title)',
      remNone: 'Nothing scheduled', remNoneAhead: 'Nothing coming up', remAdd: 'Add', remLocal: 'Local', remGoogle: 'Google Calendar', remMore: '+{n} more', remNow: 'now',
      remInMin: 'in {n} min', remInHour: 'in {n} h', remInHM: 'in {h} h {m} min', remInDays: 'in {n} days', remAgo: '{n} min ago', remAgoH: '{n} h ago', remSnoozed: 'snoozed until {t}',
      remSnooze: 'Snooze', remDelete: 'Delete', remCancel: 'Cancel', remEdit: 'Edit', remOpen: 'Open in Google', remDoneBtn: 'Mark done', remUndo: 'Undo done',
      remReadOnly: 'This calendar is read-only', remReadOnlyEvent: "This event can't be changed", remSaving: 'Saving to Google…', remSyncing: 'Syncing…', remSynced: 'Google', remSyncFail: "Google didn't sync", remLastSync: 'Last synced',
      remSyncAuth: 'Google needs to be connected again', remSyncScope: 'Google access is missing a permission', remSyncRate: 'Google is busy, try again later', remSyncOffline: 'Offline · showing the last data', remRetry: 'Try again', remConnect: 'Connect Google', remReconnect: 'Reconnect',
      remCompact: 'Compact', remComfortable: 'Comfortable', remPrayerLoading: 'Getting prayer times…', remPrayerError: "Couldn't get prayer times", remPrayerPast: 'Passed', remPrayerToday: "Today's prayer times", remPrayerSuggest: 'Show prayer times for {city}', remPrayerOn: 'Turn on',
      homeNoNext: 'Nothing coming up', homeAddHint: 'Add a reminder on the Reminders page', homeTasks: '{n} tasks running', homeNoTasks: 'No tasks', homeTasksHint: 'Background tasks show here', homeTimerRun: 'Running', homeFocusHint: 'Pick a folder or a work context', homeMusicHint: 'Play a song, its controls are here',
      wxFeels: 'feels {t}°', wxSetup: 'Enter your city',remPrayerGuide: 'Pick your city so prayer times are accurate', remPrayerSetupBtn: 'Set up', remPrayerHide: 'Hide',
      remNotifyBefore: 'Notify {n} min before', remNotifyAt: 'Notify on time', remNotifyOff: 'Notifications off',
      prayer_imsak: 'Imsak', prayer_subuh: 'Fajr', prayer_terbit: 'Sunrise', prayer_dhuha: 'Duha', prayer_dzuhur: 'Dhuhr', prayer_ashar: 'Asr', prayer_maghrib: 'Maghrib', prayer_isya: 'Isha',
    },
  },
};

function lang() {
  const l = config.load().language;
  return LANGS[l] ? l : 'id';
}

// The language of the user's last question, guessed from its words (null if it can't tell).
const ID_WORDS = new Set('yang ini itu apa aku kamu gimana bagaimana di ke dari dan tidak nggak gak ga ada bisa tolong buka mana kenapa berapa sih dong ya juga untuk dengan sudah udah lagi jam saya mau bikin cara tombol jelasin coba kok deh nih tuh'.split(' '));
const EN_WORDS = new Set('the is are what how where why this that i you can please open my me to of and in on it do does a an with for button time which show tell explain help could would should'.split(' '));
function detectLang(text) {
  let id = 0;
  let en = 0;
  for (const w of String(text || '').toLowerCase().match(/[a-z']+/g) || []) {
    if (ID_WORDS.has(w)) id++;
    if (EN_WORDS.has(w)) en++;
  }
  return id > en ? 'id' : en > id ? 'en' : null;
}

let spoken = null;
function setSpokenLang(l) {
  if (l) spoken = l;
}

// The language for what Badi shows and says around an answer: the fixed language, or with 'auto'
// the language the user last spoke ('id' until known).
function uiLang() {
  const l = lang();
  return l === 'auto' ? spoken || 'id' : l;
}

// App messages: English only when English is chosen.
function t(key, ...args) {
  const table = STRINGS[uiLang() === 'en' ? 'en' : 'id'];
  const v = table[key];
  return typeof v === 'function' ? v(...args) : v;
}

// Added to every system prompt so Badi (and its background agents) answer in the chosen language.
function languageInstruction() {
  const l = lang();
  if (l === 'auto') return 'Language: reply in the language the user is speaking or writing in; text you write on screen uses that language too.';
  const name = LANGS[l].name;
  return `Language: always reply in ${name}, including any text you write on screen, unless the user explicitly asks for another language. Keep technical names (apps, buttons, menus) exactly as they appear on screen.`;
}

module.exports = { t, lang, uiLang, detectLang, setSpokenLang, languageInstruction, LANGS, STRINGS };
