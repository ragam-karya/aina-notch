// Badi's input: the global keyboard and mouse (hooks), hold-to-talk detection, injected input for
// the Brain, key-name parsing, and what the user's mouse movement means while they talk.

pub mod gesture;
pub mod hotkeys;
pub mod keys;

#[cfg(windows)]
pub mod hook;
#[cfg(windows)]
pub mod send;
