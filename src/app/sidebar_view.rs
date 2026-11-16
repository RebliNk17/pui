//! Durable sidebar presentation only. Never changes execution or permission state.
use super::*;
mod menu;

pub(super) fn demo_menu(ctx: &egui::Context) {
    menu::demo(ctx);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Group {
    Date,
    Status,
    #[default]
    Folder,
    Environment,
    None,
}
impl Group {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Date => "Date",
            Self::Status => "Status",
            Self::Folder => "Folder",
            Self::Environment => "Environment",
            Self::None => "None",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Options {
    pub group: Group,
    pub metadata: bool,
    pub scheduled: bool,
    pub side_chats: bool,
    pub empty_groups: bool,
    pub view_all: bool,
    pub sections: Vec<sidebar::sections::Section>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            group: Group::Folder,
            metadata: false,
            scheduled: true,
            side_chats: true,
            empty_groups: false,
            view_all: false,
            sections: Vec::new(),
        }
    }
}

impl Pui {
    pub(super) fn sidebar_status(&self, branch: &sidebar_tree::Branch) -> String {
        if branch.active(self) {
            return self.run_label().0.into();
        }
        if !branch.status.is_empty() {
            return branch.status.clone();
        }
        if let Some(path) = &branch.path {
            if let Some(status) = self.other_window_status(path) {
                return status.into();
            }
        }
        "Saved · live status unknown".into()
    }

    pub(super) fn sidebar_view_options(&mut self, ui: &mut egui::Ui) -> egui::Response {
        menu::show(self, ui)
    }

    pub(super) fn sidebar_view_inputs(&mut self, ctx: &egui::Context) {
        self.unsaved_section_inputs(ctx);
        menu::inputs(self, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presentation_defaults_are_compact_and_roundtrip_without_execution() {
        let options: Options = serde_json::from_str("{}").unwrap();
        assert_eq!(options.group, Group::Folder);
        assert!(!options.metadata);
        assert!(options.scheduled && options.side_chats);
        let value = serde_json::to_value(options).unwrap();
        assert!(value.get("permission").is_none());
        assert!(value.get("prompt").is_none());
        assert!(!serde_json::from_value::<Options>(value).unwrap().view_all);
    }
}
