/// Visual cursor modes for gameplay navigation and interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameCursorKind {
    Default,
    MoveIdle,
    MovePressed,
    Interact,
    InteractPressed,
    Blocked,
}

const CURSOR_MOVE_IDLE: &str = "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='32' height='32' viewBox='0 0 32 32'%3E%3Cg stroke='%23031322' fill='%23031322' stroke-width='2.4' stroke-linejoin='round'%3E%3Cpath d='M16 11 L11.5 3.5 L16 5.7 L20.5 3.5 Z'/%3E%3Cpath d='M16 21 L11.5 28.5 L16 26.3 L20.5 28.5 Z'/%3E%3Cpath d='M11 16 L3.5 11.5 L5.7 16 L3.5 20.5 Z'/%3E%3Cpath d='M21 16 L28.5 11.5 L26.3 16 L28.5 20.5 Z'/%3E%3C/g%3E%3Cpath d='M16 11 L11.5 3.5 L16 5.7 Z' fill='%239ef9ff'/%3E%3Cpath d='M16 11 L16 5.7 L20.5 3.5 Z' fill='%2300bfe0'/%3E%3Cpath d='M16 21 L11.5 28.5 L16 26.3 Z' fill='%238ef7ff'/%3E%3Cpath d='M16 21 L16 26.3 L20.5 28.5 Z' fill='%23009ec2'/%3E%3Cpath d='M11 16 L3.5 11.5 L5.7 16 Z' fill='%239ef9ff'/%3E%3Cpath d='M11 16 L5.7 16 L3.5 20.5 Z' fill='%2300b8db'/%3E%3Cpath d='M21 16 L28.5 11.5 L26.3 16 Z' fill='%238ef7ff'/%3E%3Cpath d='M21 16 L26.3 16 L28.5 20.5 Z' fill='%23009ec2'/%3E%3Cg stroke='%23e8feff' stroke-width='0.75' stroke-linecap='round' stroke-linejoin='round' fill='none'%3E%3Cpath d='M11.5 3.5 L16 11 L20.5 3.5'/%3E%3Cpath d='M11.5 28.5 L16 21 L20.5 28.5'/%3E%3Cpath d='M3.5 11.5 L11 16 L3.5 20.5'/%3E%3Cpath d='M28.5 11.5 L21 16 L28.5 20.5'/%3E%3C/g%3E%3C/svg%3E\") 16 16, crosshair";

const CURSOR_MOVE_PRESSED: &str = "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='32' height='32' viewBox='0 0 32 32'%3E%3Cg stroke='%23031322' fill='%23031322' stroke-width='2.4' stroke-linejoin='round'%3E%3Cpath d='M16 13.2 L11.8 5.7 L16 7.8 L20.2 5.7 Z'/%3E%3Cpath d='M16 18.8 L11.8 26.3 L16 24.2 L20.2 26.3 Z'/%3E%3Cpath d='M13.2 16 L5.7 11.8 L7.8 16 L5.7 20.2 Z'/%3E%3Cpath d='M18.8 16 L26.3 11.8 L24.2 16 L26.3 20.2 Z'/%3E%3C/g%3E%3Cpath d='M16 13.2 L11.8 5.7 L16 7.8 Z' fill='%23cbffff'/%3E%3Cpath d='M16 13.2 L16 7.8 L20.2 5.7 Z' fill='%231ee8ff'/%3E%3Cpath d='M16 18.8 L11.8 26.3 L16 24.2 Z' fill='%23cbffff'/%3E%3Cpath d='M16 18.8 L16 24.2 L20.2 26.3 Z' fill='%231ee8ff'/%3E%3Cpath d='M13.2 16 L5.7 11.8 L7.8 16 Z' fill='%23cbffff'/%3E%3Cpath d='M13.2 16 L7.8 16 L5.7 20.2 Z' fill='%231ee8ff'/%3E%3Cpath d='M18.8 16 L26.3 11.8 L24.2 16 Z' fill='%23cbffff'/%3E%3Cpath d='M18.8 16 L24.2 16 L26.3 20.2 Z' fill='%231ee8ff'/%3E%3Cg stroke='%239effde' stroke-width='0.8' stroke-linecap='round' stroke-linejoin='round' fill='none'%3E%3Cpath d='M11.8 5.7 L16 13.2 L20.2 5.7'/%3E%3Cpath d='M11.8 26.3 L16 18.8 L20.2 26.3'/%3E%3Cpath d='M5.7 11.8 L13.2 16 L5.7 20.2'/%3E%3Cpath d='M26.3 11.8 L18.8 16 L26.3 20.2'/%3E%3C/g%3E%3C/svg%3E\") 16 16, crosshair";

const CURSOR_INTERACT_IDLE: &str = "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='32' height='32' viewBox='0 0 32 32'%3E%3Cpath d='M8.8 15.5 L8.8 5.2 Q8.8 3 11 3 Q13.2 3 13.2 5.2 L13.2 12.2 Q13.2 10.6 15.3 10.6 Q17.4 10.6 17.4 12.5 L17.4 13.4 Q17.4 11.8 19.4 11.8 Q21.4 11.8 21.4 13.7 L21.4 14.8 Q21.4 13.3 23.2 13.3 Q25 13.3 25 15.4 L25 20.2 Q25 23.6 22.4 26.8 L21.6 28.2 L11.2 28.2 L9.6 24.8 L4.8 18.8 Q3.6 17.3 4.9 16 Q6.3 14.7 8 16.2 L8.8 17 Z' fill='%23031322' stroke='%23031322' stroke-width='2.6' stroke-linejoin='round'/%3E%3Cpath d='M8.8 15.5 L8.8 5.2 Q8.8 3 11 3 Q13.2 3 13.2 5.2 L13.2 12.2 Q13.2 10.6 15.3 10.6 Q17.4 10.6 17.4 12.5 L17.4 13.4 Q17.4 11.8 19.4 11.8 Q21.4 11.8 21.4 13.7 L21.4 14.8 Q21.4 13.3 23.2 13.3 Q25 13.3 25 15.4 L25 20.2 Q25 23.6 22.4 26.8 L21.6 28.2 L11.2 28.2 L9.6 24.8 L4.8 18.8 Q3.6 17.3 4.9 16 Q6.3 14.7 8 16.2 L8.8 17 Z' fill='%2300bfe0'/%3E%3Cpath d='M11 3 Q8.8 3 8.8 5.2 L8.8 17 L8 16.2 Q6.3 14.7 4.9 16 Q3.6 17.3 4.8 18.8 L9.6 24.8 L11.2 28.2 L15.4 28.2 L13.2 15.8 L11.4 12.2 Z' fill='%239ef9ff'/%3E%3Cpath d='M13.2 12.2 L13.2 16.2 M17.4 12.6 L17.4 16.6 M21.4 13.8 L21.4 17.4 M10.4 25.6 L22.4 25.6' stroke='%23031322' stroke-width='1.2' stroke-linecap='round' fill='none'/%3E%3Cpath d='M4.9 16 Q6.3 14.7 8 16.2 M8.8 14.5 L8.8 5.2 Q8.8 3 11 3 Q13.2 3 13.2 5.2 M13.7 11.2 Q15.3 10.4 16.9 11.4 M17.9 12.4 Q19.4 11.6 20.9 12.6 M21.9 13.9 Q23.2 13.1 24.5 14.2 M13.6 21.6 L16.3 19.9 L19 21.6' stroke='%23e8feff' stroke-width='0.8' stroke-linecap='round' stroke-linejoin='round' fill='none'/%3E%3C/svg%3E\") 11 3, pointer";

const CURSOR_INTERACT_PRESSED: &str = "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='32' height='32' viewBox='0 0 32 32'%3E%3Cpath d='M8.8 15.5 L8.8 5.2 Q8.8 3 11 3 Q13.2 3 13.2 5.2 L13.2 12.2 Q13.2 10.6 15.3 10.6 Q17.4 10.6 17.4 12.5 L17.4 13.4 Q17.4 11.8 19.4 11.8 Q21.4 11.8 21.4 13.7 L21.4 14.8 Q21.4 13.3 23.2 13.3 Q25 13.3 25 15.4 L25 20.2 Q25 23.6 22.4 26.8 L21.6 28.2 L11.2 28.2 L9.6 24.8 L4.8 18.8 Q3.6 17.3 4.9 16 Q6.3 14.7 8 16.2 L8.8 17 Z' fill='%23031322' stroke='%23031322' stroke-width='2.6' stroke-linejoin='round'/%3E%3Cpath d='M8.8 15.5 L8.8 5.2 Q8.8 3 11 3 Q13.2 3 13.2 5.2 L13.2 12.2 Q13.2 10.6 15.3 10.6 Q17.4 10.6 17.4 12.5 L17.4 13.4 Q17.4 11.8 19.4 11.8 Q21.4 11.8 21.4 13.7 L21.4 14.8 Q21.4 13.3 23.2 13.3 Q25 13.3 25 15.4 L25 20.2 Q25 23.6 22.4 26.8 L21.6 28.2 L11.2 28.2 L9.6 24.8 L4.8 18.8 Q3.6 17.3 4.9 16 Q6.3 14.7 8 16.2 L8.8 17 Z' fill='%231ee8ff'/%3E%3Cpath d='M11 3 Q8.8 3 8.8 5.2 L8.8 17 L8 16.2 Q6.3 14.7 4.9 16 Q3.6 17.3 4.8 18.8 L9.6 24.8 L11.2 28.2 L15.4 28.2 L13.2 15.8 L11.4 12.2 Z' fill='%23cbffff'/%3E%3Cpath d='M13.2 12.2 L13.2 16.2 M17.4 12.6 L17.4 16.6 M21.4 13.8 L21.4 17.4 M10.4 25.6 L22.4 25.6' stroke='%23031322' stroke-width='1.2' stroke-linecap='round' fill='none'/%3E%3Cpath d='M4.9 16 Q6.3 14.7 8 16.2 M8.8 14.5 L8.8 5.2 Q8.8 3 11 3 Q13.2 3 13.2 5.2 M13.7 11.2 Q15.3 10.4 16.9 11.4 M17.9 12.4 Q19.4 11.6 20.9 12.6 M21.9 13.9 Q23.2 13.1 24.5 14.2 M13.6 21.6 L16.3 19.9 L19 21.6' stroke='%239effde' stroke-width='0.8' stroke-linecap='round' stroke-linejoin='round' fill='none'/%3E%3C/svg%3E\") 11 3, pointer";

impl GameCursorKind {
    pub const fn css_value(self) -> &'static str {
        match self {
            Self::Default => "",
            Self::MoveIdle => CURSOR_MOVE_IDLE,
            Self::MovePressed => CURSOR_MOVE_PRESSED,
            Self::Interact => CURSOR_INTERACT_IDLE,
            Self::InteractPressed => CURSOR_INTERACT_PRESSED,
            Self::Blocked => "not-allowed",
        }
    }
}

/// Resolves which cursor icon should be shown based on game phase and hover/mouse state.
pub fn resolve_game_cursor(
    is_playing: bool,
    middle_mouse_down: bool,
    hovered_clickable_entity: bool,
    hover_walkable: Option<bool>,
    left_mouse_down: bool,
) -> GameCursorKind {
    if !is_playing {
        return GameCursorKind::Default;
    }
    if middle_mouse_down {
        return GameCursorKind::MoveIdle;
    }
    if hovered_clickable_entity {
        return if left_mouse_down {
            GameCursorKind::InteractPressed
        } else {
            GameCursorKind::Interact
        };
    }
    match hover_walkable {
        Some(false) => GameCursorKind::Blocked,
        Some(true) if left_mouse_down => GameCursorKind::MovePressed,
        Some(true) | None => GameCursorKind::MoveIdle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clickable_trigger_overrides_walkable_ground_and_tracks_press() {
        assert_eq!(
            resolve_game_cursor(true, false, true, Some(true), false),
            GameCursorKind::Interact
        );
        assert_eq!(
            resolve_game_cursor(true, false, true, Some(false), true),
            GameCursorKind::InteractPressed
        );
        assert_eq!(GameCursorKind::Interact.css_value(), CURSOR_INTERACT_IDLE);
        assert_eq!(
            GameCursorKind::InteractPressed.css_value(),
            CURSOR_INTERACT_PRESSED
        );
    }

    #[test]
    fn default_walkable_and_pressed_states_resolve_cleanly() {
        assert_eq!(
            resolve_game_cursor(true, false, false, Some(true), false),
            GameCursorKind::MoveIdle
        );
        assert_eq!(
            resolve_game_cursor(true, false, false, Some(true), true),
            GameCursorKind::MovePressed
        );
        assert_eq!(
            resolve_game_cursor(true, false, false, None, false),
            GameCursorKind::MoveIdle
        );
        assert_eq!(
            resolve_game_cursor(true, true, false, Some(false), false),
            GameCursorKind::MoveIdle
        );
    }

    #[test]
    fn blocked_and_non_playing_states_resolve_cleanly() {
        assert_eq!(
            resolve_game_cursor(true, false, false, Some(false), false),
            GameCursorKind::Blocked
        );
        assert_eq!(GameCursorKind::Blocked.css_value(), "not-allowed");

        assert_eq!(
            resolve_game_cursor(false, false, true, Some(true), false),
            GameCursorKind::Default
        );
        assert_eq!(GameCursorKind::Default.css_value(), "");
    }

    #[test]
    fn custom_svg_cursors_are_valid_encoded_data_urls_with_symmetric_dart_tips_and_palette() {
        let prefix = "url(\"data:image/svg+xml,";
        let move_suffix = "\") 16 16, crosshair";
        let interact_suffix = "\") 11 3, pointer";

        for (cursor, tips) in [
            (
                CURSOR_MOVE_IDLE,
                ["M16 11 ", "M16 21 ", "M11 16 ", "M21 16 "],
            ),
            (
                CURSOR_MOVE_PRESSED,
                ["M16 13.2 ", "M16 18.8 ", "M13.2 16 ", "M18.8 16 "],
            ),
        ] {
            assert!(cursor.starts_with(prefix), "missing data URL prefix");
            assert!(
                cursor.ends_with(move_suffix),
                "missing hotspot/fallback suffix"
            );
            let payload = &cursor[prefix.len()..cursor.len() - move_suffix.len()];
            assert!(!payload.contains('#'), "unescaped '#' in data URL payload");
            assert!(!payload.contains('"'), "unescaped '\"' in data URL payload");
            assert!(!payload.contains('<'), "unescaped '<' in data URL payload");
            assert!(!payload.contains('>'), "unescaped '>' in data URL payload");
            for tip in tips {
                assert!(payload.contains(tip), "missing cardinal tip '{tip}'");
            }
        }

        for (cursor, palette_tokens) in [
            (CURSOR_INTERACT_IDLE, ["%239ef9ff", "%2300bfe0"]),
            (CURSOR_INTERACT_PRESSED, ["%23cbffff", "%231ee8ff"]),
        ] {
            assert!(cursor.starts_with(prefix), "missing data URL prefix");
            assert!(
                cursor.ends_with(interact_suffix),
                "missing interact hotspot/fallback suffix"
            );
            let payload = &cursor[prefix.len()..cursor.len() - interact_suffix.len()];
            assert!(!payload.contains('#'), "unescaped '#' in data URL payload");
            assert!(!payload.contains('"'), "unescaped '\"' in data URL payload");
            assert!(!payload.contains('<'), "unescaped '<' in data URL payload");
            assert!(!payload.contains('>'), "unescaped '>' in data URL payload");
            for tok in palette_tokens {
                assert!(
                    payload.contains(tok),
                    "missing matching cyan palette token '{tok}'"
                );
            }
        }
    }
}
