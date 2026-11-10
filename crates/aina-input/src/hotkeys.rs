// Hold-to-talk detection from raw key events: which combo is held, when it ends, and when a real key
// pressed during the hold means the user is typing a shortcut, not talking. Pure state machine, fed
// by the keyboard hook (hook.rs). Port of src/hotkeys.js.
//
// Virtual keys from a low-level hook are side-specific (left/right Ctrl…). One difference from v1:
// AltGr on European layouts sends a fake left Ctrl together with right Alt; the hook drops that fake
// Ctrl (hook.rs), so typing €, ą or @ never starts aina's Ctrl+Alt.

use std::collections::HashSet;

pub const VK_ESCAPE: u16 = 0x1b;
pub const VK_RETURN: u16 = 0x0d;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mod {
    Ctrl,
    Alt,
    Shift,
    Meta,
}

impl Mod {
    pub const ALL: [Mod; 4] = [Mod::Ctrl, Mod::Alt, Mod::Shift, Mod::Meta];

    fn codes(self) -> [u16; 4] {
        // Generic and left/right virtual keys.
        match self {
            Mod::Ctrl => [0x11, 0xa2, 0xa3, 0],
            Mod::Alt => [0x12, 0xa4, 0xa5, 0],
            Mod::Shift => [0x10, 0xa0, 0xa1, 0],
            Mod::Meta => [0x5b, 0x5c, 0, 0],
        }
    }

    pub fn of(vk: u16) -> Option<Mod> {
        Mod::ALL.into_iter().find(|m| vk != 0 && m.codes().contains(&vk))
    }

    /// The names config.json uses ("Ctrl", "Alt", "Shift", "Meta").
    pub fn parse(name: &str) -> Option<Mod> {
        Some(match name {
            "Ctrl" => Mod::Ctrl,
            "Alt" => Mod::Alt,
            "Shift" => Mod::Shift,
            "Meta" => Mod::Meta,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ptt,
    Dictation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Start(Kind),
    End(Kind),
    Cancel(Kind),
    Escape,
    Enter,
}

pub struct Hotkeys {
    down: HashSet<u16>,
    active: Option<Kind>,
    cancelled: bool,
    pub ptt: Vec<Mod>,
    pub dictation: Vec<Mod>,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Hotkeys { down: HashSet::new(), active: None, cancelled: false, ptt: vec![Mod::Ctrl, Mod::Alt], dictation: vec![Mod::Ctrl, Mod::Meta] }
    }
}

impl Hotkeys {
    /// Combos from config ("pttCombo": ["Ctrl", "Alt"]); unknown names are dropped.
    pub fn configure(&mut self, ptt: &[String], dictation: &[String]) {
        let parse = |c: &[String]| c.iter().filter_map(|n| Mod::parse(n)).collect::<Vec<_>>();
        if !ptt.is_empty() {
            self.ptt = parse(ptt);
        }
        if !dictation.is_empty() {
            self.dictation = parse(dictation);
        }
    }

    fn combo(&self, kind: Kind) -> &[Mod] {
        match kind {
            Kind::Ptt => &self.ptt,
            Kind::Dictation => &self.dictation,
        }
    }

    fn held(&self, m: Mod) -> bool {
        self.down.iter().any(|vk| Mod::of(*vk) == Some(m))
    }

    // A combo matches only when exactly its modifiers are held.
    fn matches(&self, combo: &[Mod]) -> bool {
        Mod::ALL.iter().all(|m| combo.contains(m) == self.held(*m))
    }

    pub fn active(&self) -> Option<Kind> {
        self.active
    }

    pub fn key_down(&mut self, vk: u16) -> Vec<Event> {
        self.down.insert(vk);
        if vk == VK_ESCAPE {
            return vec![Event::Escape];
        }
        let mut out = Vec::new();
        if vk == VK_RETURN && self.active.is_none() {
            out.push(Event::Enter);
        }
        if Mod::of(vk).is_none() {
            // A real key pressed during the hold means the user is typing a shortcut, not talking.
            if let Some(kind) = self.active.filter(|_| !self.cancelled) {
                self.cancelled = true;
                out.push(Event::Cancel(kind));
            }
            return out;
        }
        if self.active.is_none() {
            for kind in [Kind::Ptt, Kind::Dictation] {
                if !self.combo(kind).is_empty() && self.matches(self.combo(kind)) {
                    self.active = Some(kind);
                    self.cancelled = false;
                    out.push(Event::Start(kind));
                    break;
                }
            }
        }
        out
    }

    pub fn key_up(&mut self, vk: u16) -> Vec<Event> {
        self.down.remove(&vk);
        let Some(kind) = self.active else { return vec![] };
        if Mod::of(vk).is_some() && !self.combo(kind).iter().all(|m| self.held(*m)) {
            self.active = None;
            if !self.cancelled {
                return vec![Event::End(kind)];
            }
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCTRL: u16 = 0xa2;
    const LALT: u16 = 0xa4;
    const LWIN: u16 = 0x5b;
    const KEY_S: u16 = 0x53;

    #[test]
    fn hold_ctrl_alt_starts_and_releasing_either_ends() {
        let mut h = Hotkeys::default();
        assert_eq!(h.key_down(LCTRL), vec![]);
        assert_eq!(h.key_down(LALT), vec![Event::Start(Kind::Ptt)]);
        assert_eq!(h.key_down(LALT), vec![]); // key repeat
        assert_eq!(h.key_up(LALT), vec![Event::End(Kind::Ptt)]);
        assert_eq!(h.key_up(LCTRL), vec![]);
    }

    #[test]
    fn a_real_key_during_the_hold_cancels_it() {
        let mut h = Hotkeys::default();
        h.key_down(LCTRL);
        h.key_down(LALT);
        assert_eq!(h.key_down(KEY_S), vec![Event::Cancel(Kind::Ptt)]);
        assert_eq!(h.key_up(KEY_S), vec![]);
        assert_eq!(h.key_up(LALT), vec![]); // cancelled: no end
    }

    #[test]
    fn exactly_the_combo_ctrl_alt_shift_is_not_ctrl_alt() {
        let mut h = Hotkeys::default();
        h.key_down(0xa0); // shift first
        h.key_down(LCTRL);
        assert_eq!(h.key_down(LALT), vec![]);
    }

    #[test]
    fn ctrl_win_is_dictation_and_escape_enter_pass_through() {
        let mut h = Hotkeys::default();
        h.key_down(LCTRL);
        assert_eq!(h.key_down(LWIN), vec![Event::Start(Kind::Dictation)]);
        assert_eq!(h.key_up(LCTRL), vec![Event::End(Kind::Dictation)]);
        h.key_up(LWIN);
        assert_eq!(h.key_down(VK_ESCAPE), vec![Event::Escape]);
        assert_eq!(h.key_down(VK_RETURN), vec![Event::Enter]);
    }

    #[test]
    fn combos_come_from_config() {
        let mut h = Hotkeys::default();
        h.configure(&["Ctrl".into(), "Shift".into()], &[]);
        h.key_down(LCTRL);
        assert_eq!(h.key_down(0xa1), vec![Event::Start(Kind::Ptt)]);
    }
}
