//! Pui's own Pi folder: start-up adoption, the migration offer and Settings.
use super::*;
use crate::pi_migration::{self, Candidate};

#[derive(Default)]
pub(super) struct PiFolder {
    /// Pi folders found in the default places, offered for migration.
    pub offer: Vec<Candidate>,
    pub choice: usize,
    pub open: bool,
    pub notice: Option<String>,
    pub error: Option<String>,
    /// Confirmation for copying settings again into an existing Pui folder.
    pub reimport: Option<Candidate>,
}

impl Preferences {
    /// Runs before any Pi process starts or sessions are discovered.
    pub(super) fn setup_pi_folder(&mut self) -> PiFolder {
        let mut folder = PiFolder::default();
        let Some(target) = crate::runtime::pui_agent_dir() else {
            return folder;
        };
        if crate::runtime::load_managed_agent_dir().is_some() {
            self.pi_migration_pending = None;
            return folder;
        }
        if let Some(source) = self.pi_migration_pending.take() {
            match pi_migration::migrate(&source, &target) {
                Ok(moved) => {
                    if let Some(moved) = moved {
                        self.rewrite_session_paths(&moved);
                        pi_migration::rewrite_stores(&moved);
                    }
                    crate::runtime::load_managed_agent_dir();
                    folder.notice = Some(format!(
                        "Pi now runs from Pui's own folder. Settings were copied from {}; chats were moved and a link was left there.",
                        source.display()
                    ));
                    return folder;
                }
                Err(error) => folder.error = Some(format!("Moving Pi into Pui failed: {error}")),
            }
        }
        folder.offer = pi_migration::scan(&target);
        if folder.offer.is_empty() {
            // Nothing to bring over: start with Pui's own empty Pi folder.
            if pi_migration::create_empty(&target).is_ok() {
                crate::runtime::load_managed_agent_dir();
            }
        } else {
            folder.open = !self.pi_migration_dismissed;
        }
        folder
    }

    fn rewrite_session_paths(&mut self, moved: &pi_migration::Moved) {
        let Ok(mut value) = serde_json::to_value(&*self) else {
            return;
        };
        if pi_migration::rewrite_paths(&mut value, &moved.old_sessions, &moved.new_sessions) {
            if let Ok(rewritten) = serde_json::from_value::<Preferences>(value) {
                *self = rewritten;
            }
        }
    }
}

/// Reopen Pui once this process has fully exited.
fn relaunch_after_exit() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|_| "Cannot locate Pui.")?;
    let bundle = exe
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf);
    let script = r#"
pid="$1"; target="$2"; bundle="$3"
for _ in $(seq 1 600); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
kill -0 "$pid" 2>/dev/null && exit 1
if [ "$bundle" = 1 ]; then open "$target"; else "$target" >/dev/null 2>&1 & fi
"#;
    let (target, is_bundle) = match &bundle {
        Some(app) => (app.clone(), "1"),
        None => (exe, "0"),
    };
    std::process::Command::new("/bin/bash")
        .arg("-c")
        .arg(script)
        .arg("pui-relaunch")
        .arg(std::process::id().to_string())
        .arg(target)
        .arg(is_bundle)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "Could not schedule Pui to reopen.")?;
    Ok(())
}

use std::path::Path;

impl Pui {
    pub(super) fn pi_folder_prompt(&mut self, ctx: &egui::Context) {
        self.pi_folder_reimport(ctx);
        if !self.pi_folder.open || self.pi_folder.offer.is_empty() {
            return;
        }
        let candidate =
            self.pi_folder.offer[self.pi_folder.choice.min(self.pi_folder.offer.len() - 1)].clone();
        let offer = self.pi_folder.offer.clone();
        let mut choice = self.pi_folder.choice;
        let result = confirmations::Dialog::new(
            "pi-folder-migration",
            "Move Pi into Pui?",
            "Pui can keep its own Pi providers, logins, models, defaults, skills, extensions and chat history, separate from ~/.pi/agent.",
            "Not now",
            "Move and restart",
        )
        .destructive(false)
        .width(460.0)
        .show(ctx, |ui| {
            for (index, found) in offer.iter().enumerate() {
                if prompt_ui::option(
                    ui,
                    &found.dir.display().to_string(),
                    &found.summary(),
                    index == choice,
                )
                .clicked()
                {
                    choice = index;
                }
            }
            confirmations::note(
                ui,
                "Settings are copied; the original files stay where they are. Chats are moved into Pui and a link is left at the old location, so nothing is deleted and saved chats keep opening. Pui restarts to finish, before any agent starts.",
            );
            let keys = crate::runtime::environment_key_names();
            if !keys.is_empty() {
                confirmations::note(
                    ui,
                    &format!(
                        "Your environment also sets {}. Pui will not give these to Pi as provider logins (agent shell commands still get them). Add providers in Settings \u{2192} Providers, or allow environment keys in Settings \u{2192} General.",
                        keys.join(", ")
                    ),
                );
            }
        });
        self.pi_folder.choice = choice;
        match result {
            Some(confirmations::Choice::Confirm) => {
                self.prefs.pi_migration_pending = Some(candidate.dir.clone());
                self.prefs.pi_migration_dismissed = false;
                match relaunch_after_exit() {
                    Ok(()) => {
                        self.pi_folder.open = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(error) => {
                        self.pi_folder.open = false;
                        self.pi_folder.notice =
                            Some(format!("{error} Quit and reopen Pui to finish moving Pi."));
                    }
                }
            }
            Some(confirmations::Choice::Cancel) => {
                self.pi_folder.open = false;
                self.prefs.pi_migration_dismissed = true;
            }
            None => {}
        }
    }

    fn pi_folder_reimport(&mut self, ctx: &egui::Context) {
        if let Some(found) = self.pi_folder.reimport.clone() {
            let target = crate::runtime::managed_agent_dir();
            let result = confirmations::Dialog::new(
                "pi-folder-reimport",
                "Copy settings from Pi again?",
                "Providers, logins, models, defaults, skills and extensions from this folder replace the matching files in Pui. Chats are not touched.",
                "Cancel",
                "Copy into Pui",
            )
            .width(440.0)
            .show(ctx, |ui| {
                confirmations::note(ui, &format!("{}\n{}", found.dir.display(), found.summary()));
            });
            match result {
                Some(confirmations::Choice::Confirm) => {
                    self.pi_folder.reimport = None;
                    match target
                        .ok_or("Pui's Pi folder is unavailable.".to_owned())
                        .and_then(|target| pi_migration::import_settings(&found.dir, &target))
                    {
                        Ok(()) => {
                            self.pi_folder.notice = Some(
                                "Copied Pi's settings into Pui. Chats pick them up automatically."
                                    .into(),
                            )
                        }
                        Err(error) => self.pi_folder.error = Some(error),
                    }
                }
                Some(confirmations::Choice::Cancel) => self.pi_folder.reimport = None,
                None => {}
            }
        }
    }

    pub(super) fn settings_pi_folder(&mut self, ui: &mut egui::Ui) {
        page_section(
            ui,
            "Pi data",
            "Where Pui keeps Pi's providers, logins, models, defaults, skills, extensions and chats.",
        );
        let managed = crate::runtime::managed_agent_dir();
        let colors = theme::palette(ui.ctx());
        settings_card(ui, |ui| {
            let location = managed
                .clone()
                .or_else(crate::runtime::agent_dir)
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            settings_row(
                ui,
                if managed.is_some() {
                    "Pui's Pi folder"
                } else {
                    "Shared Pi folder"
                },
                &location,
                |ui| {
                    if managed.is_some() {
                        if ui.button("Copy from Pi\u{2026}").clicked() {
                            let target = managed.clone().unwrap_or_default();
                            match pi_migration::scan(&target).into_iter().next() {
                                Some(found) => self.pi_folder.reimport = Some(found),
                                None => {
                                    self.pi_folder.notice =
                                        Some("No other Pi folder was found to copy from.".into())
                                }
                            }
                        }
                    } else if ui.button("Move into Pui\u{2026}").clicked() {
                        if let Some(target) = crate::runtime::pui_agent_dir() {
                            self.pi_folder.offer = pi_migration::scan(&target);
                            self.pi_folder.choice = 0;
                            self.pi_folder.open = !self.pi_folder.offer.is_empty();
                        }
                    }
                },
            );
            settings_separator(ui);
            let keys = crate::runtime::environment_key_names();
            let description = if keys.is_empty() {
                "Off: Pi only uses logins and providers saved in Pui. No provider keys were found in your environment.".to_owned()
            } else {
                format!(
                    "Found {}. Off: Pi ignores them as provider logins (a key under the wrong name can't pose as another provider); agent shell commands still receive them. Applies to chats started afterwards.",
                    keys.join(", ")
                )
            };
            settings_row(
                ui,
                "Use provider keys from environment",
                &description,
                |ui| {
                    let mut enabled = self.prefs.use_environment_keys;
                    super::settings::toggle_switch(
                        ui,
                        &mut enabled,
                        "Use provider keys from environment",
                    );
                    if enabled != self.prefs.use_environment_keys {
                        self.prefs.use_environment_keys = enabled;
                        crate::runtime::set_use_environment_keys(enabled);
                        self.catalog.request(&self.ctx);
                    }
                },
            );
        });
        for (text, color) in [
            (self.pi_folder.notice.clone(), colors.green),
            (self.pi_folder.error.clone(), colors.red),
        ] {
            if let Some(text) = text {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(text).size(11.0).color(color)).wrap());
            }
        }
    }
}

fn page_section(ui: &mut egui::Ui, title: &str, description: &str) {
    super::settings::page_section(ui, title, description);
}
fn settings_card(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    super::settings::page_card(ui, body);
}
fn settings_row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    super::settings::page_row(ui, title, description, body);
}
fn settings_separator(ui: &mut egui::Ui) {
    super::settings::page_separator(ui);
}
