// Parses the key strings the computer toolset sends ("Return", "ctrl+s", "alt+Tab", "super") into
// Windows virtual-key codes. Pure. Port of src/brain/keys.js.

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Vk(u16),
    /// Any other single character: typed by its Unicode value, or looked up for a shortcut.
    Char(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Combo {
    pub mods: Vec<u16>,
    pub key: Key,
}

pub fn modifier(name: &str) -> Option<u16> {
    Some(match norm(name).as_str() {
        "ctrl" | "control" => 0x11,
        "shift" => 0x10,
        "alt" | "option" => 0x12,
        "super" | "win" | "windows" | "meta" | "cmd" | "command" => 0x5b,
        _ => return None,
    })
}

// Keys that need KEYEVENTF_EXTENDEDKEY so Windows doesn't read them as numpad keys.
pub fn is_extended(vk: u16) -> bool {
    matches!(vk, 0x21 | 0x22 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 | 0x2d | 0x2e | 0x5b | 0x5c | 0x5d | 0x6f | 0x90 | 0x2c)
}

fn named(n: &str) -> Option<u16> {
    Some(match n {
        "return" | "enter" | "kp_enter" => 0x0d,
        "tab" | "iso_left_tab" => 0x09,
        "escape" | "esc" => 0x1b,
        "backspace" => 0x08,
        "delete" | "del" => 0x2e,
        "insert" => 0x2d,
        "space" => 0x20,
        "home" => 0x24,
        "end" => 0x23,
        "page_up" | "prior" | "pageup" => 0x21,
        "page_down" | "next" | "pagedown" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "print" | "printscreen" => 0x2c,
        "menu" | "apps" => 0x5d,
        "caps_lock" | "capslock" => 0x14,
        "num_lock" => 0x90,
        "scroll_lock" => 0x91,
        "pause" => 0x13,
        "minus" => 0xbd,
        "equal" | "plus" => 0xbb,
        "comma" => 0xbc,
        "period" => 0xbe,
        "slash" => 0xbf,
        "semicolon" => 0xba,
        "apostrophe" => 0xde,
        "grave" => 0xc0,
        "backslash" => 0xdc,
        "bracketleft" => 0xdb,
        "bracketright" => 0xdd,
        "kp_add" => 0x6b,
        "kp_subtract" => 0x6d,
        "kp_multiply" => 0x6a,
        "kp_divide" => 0x6f,
        "kp_decimal" => 0x6e,
        "audiomute" | "volume_mute" => 0xad,
        "audiolowervolume" | "volume_down" => 0xae,
        "audioraisevolume" | "volume_up" => 0xaf,
        "audionext" => 0xb0,
        "audioprev" => 0xb1,
        "audiostop" => 0xb2,
        "audioplay" | "audiopause" | "media_play_pause" => 0xb3,
        _ => return None,
    })
}

// "BackSpace" -> "backspace", "Page_Down" -> "page_down", "XF86AudioPlay" -> "audioplay".
fn norm(s: &str) -> String {
    let n = s.trim().to_lowercase();
    n.strip_prefix("xf86").map(String::from).unwrap_or(n)
}

fn key_for(name: &str) -> Option<Key> {
    let n = norm(name);
    if let Some(vk) = named(&n) {
        return Some(Key::Vk(vk));
    }
    if let Some(f) = n.strip_prefix('f').and_then(|d| d.parse::<u16>().ok()).filter(|f| (1..=24).contains(f)) {
        return Some(Key::Vk(0x6f + f));
    }
    if let Some(d) = n.strip_prefix("kp_").and_then(|d| d.parse::<u16>().ok()).filter(|d| *d <= 9) {
        return Some(Key::Vk(0x60 + d));
    }
    let mut chars = n.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && (c.is_ascii_lowercase() || c.is_ascii_digit()) {
            return Some(Key::Vk(c.to_ascii_uppercase() as u16));
        }
    let t = name.trim();
    (t.chars().count() == 1).then(|| Key::Char(t.to_string()))
}

/// Returns { mods, key } or an error message the model can act on.
pub fn parse_combo(text: &str) -> Result<Combo, String> {
    if text.trim().is_empty() {
        return Err("key: \"text\" must name a key, e.g. \"Return\" or \"ctrl+s\".".into());
    }
    // A lone "+" is the plus key, not a separator.
    let parts: Vec<&str> = if text.trim() == "+" { vec!["+"] } else { text.split('+').map(str::trim).filter(|p| !p.is_empty()).collect() };
    let (last, rest) = parts.split_last().ok_or("key: \"text\" must name a key, e.g. \"Return\" or \"ctrl+s\".")?;
    let mut mods = Vec::new();
    for p in rest {
        mods.push(modifier(p).ok_or_else(|| format!("key: unknown modifier \"{p}\"."))?);
    }
    if let Some(vk) = modifier(last) {
        return Ok(Combo { mods, key: Key::Vk(vk) });
    }
    let key = key_for(last).ok_or_else(|| format!("key: unknown key \"{last}\"."))?;
    Ok(Combo { mods, key })
}

/// Modifier names for click/scroll "text" ("shift", "ctrl+alt").
pub fn parse_modifiers(text: Option<&str>) -> Result<Vec<u16>, String> {
    match text {
        None | Some("") => Ok(Vec::new()),
        Some(t) => t.split('+').map(|p| modifier(p).ok_or_else(|| format!("unknown modifier \"{p}\"."))).collect(),
    }
}

// Same cases as the key tests in test/brain-input.test.js.
#[cfg(test)]
mod tests {
    use super::*;

    fn c(mods: &[u16], key: Key) -> Combo {
        Combo { mods: mods.to_vec(), key }
    }

    #[test]
    fn parse_combo_maps_xdotool_style_keys_to_virtual_keys() {
        assert_eq!(parse_combo("Return"), Ok(c(&[], Key::Vk(0x0d))));
        assert_eq!(parse_combo("ctrl+s"), Ok(c(&[0x11], Key::Vk(0x53))));
        assert_eq!(parse_combo("alt+Tab"), Ok(c(&[0x12], Key::Vk(0x09))));
        assert_eq!(parse_combo("ctrl+shift+Escape"), Ok(c(&[0x11, 0x10], Key::Vk(0x1b))));
        assert_eq!(parse_combo("super"), Ok(c(&[], Key::Vk(0x5b))));
        assert_eq!(parse_combo("Page_Down"), Ok(c(&[], Key::Vk(0x22))));
        assert_eq!(parse_combo("F5"), Ok(c(&[], Key::Vk(0x74))));
        assert_eq!(parse_combo("BackSpace"), Ok(c(&[], Key::Vk(0x08))));
        assert_eq!(parse_combo("XF86AudioPlay"), Ok(c(&[], Key::Vk(0xb3))));
        assert_eq!(parse_combo("+"), Ok(c(&[], Key::Char("+".into()))));
        assert_eq!(parse_combo("ctrl+/"), Ok(c(&[0x11], Key::Char("/".into()))));
        assert!(parse_combo("hyper+x").unwrap_err().contains("unknown modifier"));
        assert!(parse_combo("Launch_Rocket").unwrap_err().contains("unknown key"));
        assert!(parse_combo("").unwrap_err().contains("must name a key"));
    }

    #[test]
    fn parse_modifiers_reads_click_modifiers() {
        assert_eq!(parse_modifiers(Some("shift")), Ok(vec![0x10]));
        assert_eq!(parse_modifiers(Some("ctrl+alt")), Ok(vec![0x11, 0x12]));
        assert_eq!(parse_modifiers(None), Ok(vec![]));
        assert!(parse_modifiers(Some("fn")).unwrap_err().contains("unknown modifier"));
    }
}
