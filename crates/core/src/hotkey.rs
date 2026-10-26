//! The show/hide hotkey in the Win32 hotkey-control encoding (`settings::DEFAULT_HOTKEY`): naming it for the
//! settings window and building it from a key press.

const SHIFT: u32 = 0x1;
const CTRL: u32 = 0x2;
const ALT: u32 = 0x4;

/// Modifiers held while a key goes down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Modifiers {
    fn names(self) -> impl Iterator<Item = &'static str> {
        [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| n)
    }
}

/// "Ctrl + Alt + M"; "None" for 0.
pub fn name(packed: u32) -> String {
    if packed == 0 {
        return "None".into();
    }
    let flags = packed >> 8;
    let modifiers = Modifiers { ctrl: flags & CTRL != 0, alt: flags & ALT != 0, shift: flags & SHIFT != 0 };
    modifiers.names().map(str::to_string).chain([key_name(packed & 0xFF)]).collect::<Vec<_>>().join(" + ")
}

/// What is held so far while the player builds a combo: "Ctrl + Alt + …".
pub fn partial(modifiers: Modifiers) -> String {
    modifiers.names().chain(["…"]).collect::<Vec<_>>().join(" + ")
}

/// The hotkey a key press makes, or None while only modifiers are down or when the combo is not allowed: a bare
/// key or Shift + key would fire while the player types in the game's chat.
pub fn from_press(vk: u32, modifiers: Modifiers) -> Option<u32> {
    if is_modifier(vk) || !(modifiers.ctrl || modifiers.alt) || vk > 0xFF {
        return None;
    }
    let flags = if modifiers.ctrl { CTRL } else { 0 }
        | if modifiers.alt { ALT } else { 0 }
        | if modifiers.shift { SHIFT } else { 0 };
    Some((flags << 8) | vk)
}

pub fn is_modifier(vk: u32) -> bool {
    // Shift, Ctrl, Alt (either side) and the Windows keys.
    matches!(vk, 0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C)
}

fn key_name(vk: u32) -> String {
    let named = match vk {
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x13 => "Pause",
        0x14 => "Caps Lock",
        0x1B => "Esc",
        0x20 => "Space",
        0x21 => "Page Up",
        0x22 => "Page Down",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2D => "Insert",
        0x2E => "Delete",
        0x6A => "Num *",
        0x6B => "Num +",
        0x6D => "Num -",
        0x6E => "Num .",
        0x6F => "Num /",
        0x90 => "Num Lock",
        0x91 => "Scroll Lock",
        0xBA => ";",
        0xBB => "=",
        0xBC => ",",
        0xBD => "-",
        0xBE => ".",
        0xBF => "/",
        0xC0 => "`",
        0xDB => "[",
        0xDC => "\\",
        0xDD => "]",
        0xDE => "'",
        _ => "",
    };
    match vk {
        _ if !named.is_empty() => named.into(),
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => format!("Key {vk:#04X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::DEFAULT_HOTKEY;

    const CTRL_ALT: Modifiers = Modifiers { ctrl: true, alt: true, shift: false };

    #[test]
    fn names_read_like_the_keyboard() {
        assert_eq!(name(DEFAULT_HOTKEY), "Ctrl + Alt + M");
        assert_eq!(name(0), "None");
        assert_eq!(name((SHIFT | CTRL) << 8 | 0x70), "Ctrl + Shift + F1");
        assert_eq!(name(ALT << 8 | 0x63), "Alt + Num 3");
        assert_eq!(name(CTRL << 8 | 0xBF), "Ctrl + /");
        assert_eq!(name(CTRL << 8 | 0xE7), "Ctrl + Key 0xE7");
    }

    #[test]
    fn a_press_becomes_the_packed_hotkey() {
        assert_eq!(from_press(u32::from(b'M'), CTRL_ALT), Some(DEFAULT_HOTKEY));
        assert_eq!(from_press(0x70, Modifiers { alt: true, shift: true, ..Modifiers::default() }), Some(0x570));
    }

    #[test]
    fn keys_that_would_fire_while_typing_are_refused() {
        assert_eq!(from_press(u32::from(b'M'), Modifiers::default()), None);
        assert_eq!(from_press(u32::from(b'M'), Modifiers { shift: true, ..Modifiers::default() }), None);
        assert_eq!(from_press(0x11, CTRL_ALT), None, "only modifiers held so far");
        assert_eq!(partial(CTRL_ALT), "Ctrl + Alt + …");
    }
}
