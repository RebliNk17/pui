//! User-authored progress records, never executable task state or verified evidence.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub text: String,
    pub phase: Phase,
    pub created: u64,
    #[serde(default)]
    pub run: Option<String>,
}
impl Task {
    pub fn checkpoint_summary(&self) -> Option<String> {
        let note = self.checkpoints.last()?;
        let text = note
            .text
            .chars()
            .take(120)
            .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
            .collect::<String>();
        Some(format!(
            "User note · {text}{}",
            if note.text.chars().nth(120).is_some() {
                "…"
            } else {
                ""
            }
        ))
    }
    pub fn add_checkpoint(&mut self, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty()
            || text.len() > 2048
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err(
                "Checkpoint requires 1–2,048 UTF-8 bytes without unsupported controls.".into(),
            );
        }
        if self.checkpoints.len() >= 32 {
            return Err("This task already has 32 checkpoints.".into());
        }
        self.checkpoints.push(Checkpoint {
            text: text.into(),
            phase: self.phase,
            created: now(),
            run: None,
        });
        Ok(())
    }
    pub fn add_linked_checkpoint(&mut self, text: &str, run: &str) -> Result<(), String> {
        if run.is_empty()
            || run.len() > 128
            || run.chars().any(char::is_control)
            || self.runs.iter().filter(|r| r.id == run).count() != 1
        {
            return Err("Recorded stage is missing or ambiguous; note was not saved.".into());
        }
        self.add_checkpoint(text)?;
        self.checkpoints.last_mut().unwrap().run = Some(run.into());
        Ok(())
    }
    pub fn edit_checkpoint(
        &mut self,
        index: usize,
        original: &Checkpoint,
        text: &str,
    ) -> Result<(), String> {
        if self.checkpoints.get(index) != Some(original) {
            return Err("Checkpoint changed; reopen the editor.".into());
        }
        let text = text.trim();
        if text.is_empty()
            || text.len() > 2048
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err(
                "Checkpoint requires 1–2,048 UTF-8 bytes without unsupported controls.".into(),
            );
        }
        self.checkpoints[index].text = text.into();
        Ok(())
    }
    pub fn validate_checkpoints(&self) -> Result<(), String> {
        if self.checkpoints.len() > 32
            || self.checkpoints.iter().any(|c| {
                c.run.as_ref().is_some_and(|run| {
                    run.is_empty() || run.len() > 128 || run.chars().any(char::is_control)
                }) || c.text.is_empty()
                    || c.text.len() > 2048
                    || c.text
                        .chars()
                        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
            })
        {
            return Err("Invalid or excessive task checkpoints.".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;
