use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TranslationMode {
    Conversational,
    Academic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleSelectionIcon,
    SetSelectionIcon(bool),
    SelectMode(TranslationMode),
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub selection_icon_enabled: bool,
    pub mode: TranslationMode,
    pub revision: u64,
    pub exiting: bool,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            selection_icon_enabled: true,
            mode: TranslationMode::Conversational,
            revision: 0,
            exiting: false,
        }
    }
}

impl Snapshot {
    /// Called serially on the window thread. The preview has no persisted state.
    pub fn apply(&mut self, action: Action) -> bool {
        if self.exiting {
            return false;
        }
        match action {
            Action::ToggleSelectionIcon => {
                self.selection_icon_enabled = !self.selection_icon_enabled;
            }
            Action::SetSelectionIcon(enabled) => {
                if self.selection_icon_enabled == enabled {
                    return false;
                }
                self.selection_icon_enabled = enabled;
            }
            Action::SelectMode(mode) => {
                if self.mode == mode {
                    return false;
                }
                self.mode = mode;
            }
            Action::Exit => self.exiting = true,
        }
        self.revision += 1;
        true
    }
}

/// Down and up form one click; secondary clicks only belong to the native menu.
pub fn toggles_selection(primary_button: bool, released: bool) -> bool {
    primary_button && released
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_release_toggles_once_and_leaves_mode_unchanged() {
        let mut state = Snapshot::default();
        for released in [false, true] {
            if toggles_selection(true, released) {
                state.apply(Action::ToggleSelectionIcon);
            }
        }
        assert!(!state.selection_icon_enabled);
        assert_eq!(state.mode, TranslationMode::Conversational);
        assert_eq!(state.revision, 1);
    }

    #[test]
    fn rapid_primary_clicks_are_independent_of_double_click_timing() {
        let mut state = Snapshot::default();
        for count in 1..=20 {
            state.apply(Action::ToggleSelectionIcon);
            assert_eq!(state.selection_icon_enabled, count % 2 == 0);
            assert_eq!(state.mode, TranslationMode::Conversational);
        }
        assert_eq!(state.revision, 20);
    }

    #[test]
    fn secondary_clicks_do_not_change_selection_state() {
        assert!(!toggles_selection(false, false));
        assert!(!toggles_selection(false, true));
    }

    #[test]
    fn mode_selection_is_explicit_and_idempotent() {
        let mut state = Snapshot::default();
        state.apply(Action::SetSelectionIcon(false));
        assert!(state.apply(Action::SelectMode(TranslationMode::Academic)));
        assert!(!state.apply(Action::SelectMode(TranslationMode::Academic)));
        assert_eq!(state.mode, TranslationMode::Academic);
        assert!(!state.selection_icon_enabled);
        assert_eq!(state.revision, 2);
    }

    #[test]
    fn menu_and_primary_click_share_the_latest_selection_state() {
        let mut state = Snapshot::default();
        state.apply(Action::ToggleSelectionIcon);
        assert!(!state.apply(Action::SetSelectionIcon(false)));
        state.apply(Action::SetSelectionIcon(true));
        state.apply(Action::ToggleSelectionIcon);
        assert!(!state.selection_icon_enabled);
        assert_eq!(state.revision, 3);
    }

    #[test]
    fn exiting_rejects_late_clicks_and_menu_actions() {
        let mut state = Snapshot::default();
        state.apply(Action::Exit);
        let stopped = state;
        for action in [
            Action::ToggleSelectionIcon,
            Action::SetSelectionIcon(false),
            Action::SelectMode(TranslationMode::Academic),
            Action::Exit,
        ] {
            assert!(!state.apply(action));
            assert_eq!(state, stopped);
        }
    }
}
