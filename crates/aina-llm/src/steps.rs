// Splits a streaming reply into playback steps: the visual tags that come before (or inside) a
// sentence, plus that sentence. Each step is shown and spoken together so voice and visuals stay in
// sync. Port of src/steps.js.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::tags::{Tag, parse_tag};

static TAG_AT_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[(?:POINT|HIGHLIGHT|SHAPE|TARGET|HOVER|DELEGATE|DO|FOCUS|DROP|TIMER|REMIND|MEDIA|WRITE|DRAW):[^\]]*\]").unwrap()
});
// A prefix that could still become a tag once more text arrives.
static MAYBE_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[(?:P|PO|POI|POIN|POINT|H|HI|HIG|HIGH|HIGHL|HIGHLI|HIGHLIG|HIGHLIGH|HIGHLIGHT|S|SH|SHA|SHAP|SHAPE|T|TA|TAR|TARG|TARGE|TARGET|HO|HOV|HOVE|HOVER|D|DO|DE|DEL|DELE|DELEG|DELEGA|DELEGAT|DELEGATE|DR|DRA|DRAW|W|WR|WRI|WRIT|WRITE|F|FO|FOC|FOCU|FOCUS|DRO|DROP|TI|TIM|TIME|TIMER|R|RE|REM|REMI|REMIN|REMIND|M|ME|MED|MEDI|MEDIA)?(?::[^\]]*)?$").unwrap()
});
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub tags: Vec<Tag>,
    pub text: String,
}

#[derive(Default)]
pub struct StepSplitter {
    buf: String,
    tags: Vec<Tag>,
    text: String,
}

impl StepSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a text delta; returns the steps that are now complete.
    pub fn push(&mut self, delta: &str) -> Vec<Step> {
        self.buf.push_str(delta);
        self.drain(false)
    }

    /// Ends the stream; returns whatever is left as final steps.
    pub fn finish(&mut self) -> Vec<Step> {
        let mut steps = self.drain(true);
        steps.extend(self.take());
        steps
    }

    fn take(&mut self) -> Option<Step> {
        let text = SPACES.replace_all(&self.text, " ").trim().to_string();
        if text.is_empty() && self.tags.is_empty() {
            return None;
        }
        self.text.clear();
        Some(Step { tags: std::mem::take(&mut self.tags), text })
    }

    fn drain(&mut self, last: bool) -> Vec<Step> {
        let mut steps = Vec::new();
        while let Some(ch) = self.buf.chars().next() {
            if ch == '[' {
                if let Some(end) = TAG_AT_START.find(&self.buf).map(|m| m.end()) {
                    self.tags.extend(parse_tag(&self.buf[..end]));
                    self.buf.drain(..end);
                    continue;
                }
                // Could still become a tag once more text arrives: wait for it.
                if !last && MAYBE_TAG.is_match(&self.buf) {
                    break;
                }
            }
            // Sentence end: punctuation followed by whitespace (or the end of the reply), or a newline.
            if matches!(ch, '.' | '!' | '?' | '…' | '\n') {
                let rest = &self.buf[ch.len_utf8()..];
                // Closing quotes/brackets right after the punctuation belong to the sentence.
                let close: String = if ch == '\n' { String::new() } else { rest.chars().take_while(|c| matches!(c, '"' | '\'' | '”' | '’' | ')')).collect() };
                let next = rest[close.len()..].chars().next();
                if next.is_none() && !last && ch != '\n' {
                    break; // need the next char to decide
                }
                self.text.push(if ch == '\n' { ' ' } else { ch });
                self.text.push_str(&close);
                self.buf.drain(..ch.len_utf8() + close.len());
                if (ch == '\n' || next.is_none_or(char::is_whitespace)) && !self.text.trim().is_empty() {
                    steps.extend(self.take());
                }
                continue;
            }
            self.text.push(ch);
            self.buf.drain(..ch.len_utf8());
        }
        steps
    }
}

// Same cases as test/steps.test.js.
#[cfg(test)]
mod tests {
    use super::*;

    fn kind(t: &Tag) -> String {
        let v = serde_json::to_value(t).unwrap();
        let ty = v["type"].as_str().unwrap().to_string();
        match v.get("shape").and_then(|s| s.as_str()) {
            Some(shape) => format!("{ty}:{shape}"),
            None => ty,
        }
    }

    // Feeds text in small chunks, like a stream, and collects all steps.
    fn run(text: &str, chunk: usize) -> Vec<(Vec<String>, String)> {
        let mut s = StepSplitter::new();
        let chars: Vec<char> = text.chars().collect();
        let mut steps = Vec::new();
        for piece in chars.chunks(chunk) {
            steps.extend(s.push(&piece.iter().collect::<String>()));
        }
        steps.extend(s.finish());
        steps.into_iter().map(|st| (st.tags.iter().map(kind).collect(), st.text)).collect()
    }

    fn step(tags: &[&str], text: &str) -> (Vec<String>, String) {
        (tags.iter().map(|t| t.to_string()).collect(), text.into())
    }

    #[test]
    fn tags_attach_to_the_sentence_that_follows_them() {
        assert_eq!(
            run("[SHAPE:line:1,1;2,2:red] This is the height. [WRITE:5,5,30:L = ½ × a × t] So the area is half. Done!", 3),
            vec![step(&["shape:line"], "This is the height."), step(&["write"], "So the area is half."), step(&[], "Done!")]
        );
    }

    #[test]
    fn several_leading_tags_and_one_sentence_form_one_step() {
        for chunk in [1, 2, 5, 50] {
            assert_eq!(run("[HIGHLIGHT:1,2,3,4:panel][TARGET:5,6,20:save] Click Save to keep it.", chunk), vec![step(&["highlight", "target"], "Click Save to keep it.")]);
        }
    }

    #[test]
    fn decimals_brackets_that_are_not_tags_and_closing_quotes() {
        assert_eq!(
            run("It costs 3.50 dollars [see note]. He said \"go.\" Then stop", 3),
            vec![step(&[], "It costs 3.50 dollars [see note]."), step(&[], "He said \"go.\""), step(&[], "Then stop")]
        );
    }

    #[test]
    fn tags_with_no_trailing_text_still_produce_a_step() {
        assert_eq!(run("Here. [DRAW:1,1;2,2;3,3]", 3), vec![step(&[], "Here."), step(&["draw"], "")]);
    }

    #[test]
    fn a_do_tag_streamed_in_pieces_attaches_to_its_sentence() {
        let mut s = StepSplitter::new();
        let mut steps = Vec::new();
        for piece in ["[D", "O:buka Spot", "ify] Oke, aku buka", " Spotify."] {
            steps.extend(s.push(piece));
        }
        steps.extend(s.finish());
        assert_eq!(steps, vec![Step { tags: vec![Tag::Do { task: "buka Spotify".into() }], text: "Oke, aku buka Spotify.".into() }]);
    }
}
