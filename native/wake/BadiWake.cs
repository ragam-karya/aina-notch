// "Hey aina" listener: a tiny offline helper around Windows' built-in speech recognizer (System.Speech).
// Audio never leaves the machine. The grammar only knows the wake phrases, so nothing else is transcribed.
//
// Protocol (one line each):
//   stdout: "ready" | "wake <confidence>" | "error <message>"
//   stdin:  "pause" | "resume" | "quit"   (closing stdin also quits, so it dies with the app)
// Args: [minConfidence=0.85] [--wav <file>]   (--wav reads a recording instead of the mic, for tests)
// BADI_WAKE_DEBUG=1 also prints every recognized segment as "heard ...".
using System;
using System.Globalization;
using System.Linq;
using System.Speech.Recognition;
using System.Speech.Recognition.SrgsGrammar;
using System.Threading;

static class BadiWake {
  // A wake phrase counts only when this much quiet follows it: "hello everybody, what's the weather"
  // or "hey daddy, look" run straight on, while people pause after calling a name.
  static readonly TimeSpan QuietAfter = TimeSpan.FromMilliseconds(500);

  static SpeechRecognitionEngine engine;
  static bool listening;
  static readonly object gate = new object();
  static double minConfidence = 0.85;
  static bool debug = Environment.GetEnvironmentVariable("BADI_WAKE_DEBUG") == "1";

  // A wake phrase waiting for its quiet period: where its audio ends, and its confidence.
  static TimeSpan? pendingEnd;
  static float pendingConfidence;

  static void Send(string line) {
    lock (gate) { Console.Out.WriteLine(line); Console.Out.Flush(); }
  }

  static void SetListening(bool on) {
    lock (gate) {
      if (on == listening) return;
      pendingEnd = null;
      if (on) engine.RecognizeAsync(RecognizeMode.Multiple);
      else engine.RecognizeAsyncCancel();
      listening = on;
    }
  }

  // "hey|hi|halo|hello" + "aina". The en-US recognizer has no word "aina", so its pronunciations are
  // given as SAPI phonemes: the English "EYE-nuh", the Indonesian "ah-EE-nah" and "eye-EE-nuh".
  static Grammar WakeGrammar() {
    var greeting = new SrgsOneOf("hey", "hi", "halo", "hello");
    var name = new SrgsOneOf();
    foreach (var pron in new[] { "ay n ax", "aa iy n aa", "ay iy n ax" })
      name.Add(new SrgsItem(new SrgsToken("aina") { Pronunciation = pron }));
    var root = new SrgsRule("wake");
    root.Add(greeting);
    root.Add(name);
    var doc = new SrgsDocument(root) { Culture = new CultureInfo("en-US"), PhoneticAlphabet = SrgsPhoneticAlphabet.Sapi };
    return new Grammar(doc) { Name = "wake" };
  }

  // Absorbs all other speech. Without it the recognizer forces anything onto the wake grammar.
  static Grammar OtherSpeech() {
    var root = new SrgsRule("other");
    root.Add(new SrgsItem(1, 6, SrgsRuleRef.Garbage));
    var g = new Grammar(new SrgsDocument(root)) { Name = "other" };
    float weight;
    if (float.TryParse(Environment.GetEnvironmentVariable("BADI_WAKE_OTHER_WEIGHT"), NumberStyles.Float, CultureInfo.InvariantCulture, out weight)) g.Weight = weight;
    return g;
  }

  static void OnRecognized(RecognitionResult r) {
    if (debug)
      Send("heard " + r.Grammar.Name + " " + r.Confidence.ToString("0.00", CultureInfo.InvariantCulture) + " " + r.Text +
        " words=" + string.Join(",", r.Words.Select(w => w.Text + ":" + w.Confidence.ToString("0.00", CultureInfo.InvariantCulture))));
    if (r.Grammar.Name != "wake" || r.Audio == null) return;
    var name = r.Words.LastOrDefault();
    if (r.Confidence < minConfidence || name == null || name.Confidence < minConfidence) return;
    lock (gate) {
      pendingEnd = r.Audio.AudioPosition + r.Audio.Duration;
      pendingConfidence = r.Confidence;
    }
  }

  // New speech right after the phrase cancels it.
  static void OnSpeechDetected(TimeSpan at) {
    lock (gate) {
      if (pendingEnd.HasValue && at - pendingEnd.Value < QuietAfter) pendingEnd = null;
    }
  }

  // Fires the pending wake once the quiet period has passed (or the recording ended).
  static void ConfirmIfQuiet(bool ended) {
    lock (gate) {
      if (!pendingEnd.HasValue) return;
      if (!ended && engine.RecognizerAudioPosition - pendingEnd.Value < QuietAfter) return;
      pendingEnd = null;
    }
    Send("wake " + pendingConfidence.ToString("0.00", CultureInfo.InvariantCulture));
  }

  static int Main(string[] args) {
    string wav = null;
    for (int i = 0; i < args.Length; i++) {
      if (args[i] == "--wav" && i + 1 < args.Length) wav = args[++i];
      else double.TryParse(args[i], NumberStyles.Float, CultureInfo.InvariantCulture, out minConfidence);
    }
    try {
      engine = new SpeechRecognitionEngine(new CultureInfo("en-US"));
      engine.LoadGrammar(WakeGrammar());
      if (Environment.GetEnvironmentVariable("BADI_WAKE_OTHER_WEIGHT") != "off") engine.LoadGrammar(OtherSpeech());
      engine.SpeechRecognized += (s, e) => OnRecognized(e.Result);
      engine.SpeechDetected += (s, e) => OnSpeechDetected(e.AudioPosition);
      if (wav != null) {
        engine.SetInputToWaveFile(wav);
        var finished = new ManualResetEvent(false);
        engine.RecognizeCompleted += (s, e) => finished.Set(); // fires at the end of the file
        engine.RecognizeAsync(RecognizeMode.Multiple);
        finished.WaitOne();
        ConfirmIfQuiet(true);
        Send("done");
        return 0;
      }
      engine.SetInputToDefaultAudioDevice();
      SetListening(true);
      new Timer(_ => { try { ConfirmIfQuiet(false); } catch { } }, null, 100, 100);
      Send("ready");
    } catch (Exception e) {
      Send("error " + e.Message.Replace('\n', ' '));
      return 1;
    }
    string cmd;
    while ((cmd = Console.In.ReadLine()) != null) {
      cmd = cmd.Trim();
      try {
        if (cmd == "pause") SetListening(false);
        else if (cmd == "resume") SetListening(true);
        else if (cmd == "quit") break;
      } catch (Exception e) {
        Send("error " + e.Message.Replace('\n', ' '));
      }
    }
    try { SetListening(false); engine.Dispose(); } catch { }
    return 0;
  }
}
