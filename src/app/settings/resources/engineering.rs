//! Original, explicit-invocation skills shipped with Pui; never auto-registered.
use super::*;
use sha2::{Digest, Sha256};

pub(super) struct Skill {
    pub name: &'static str,
    pub title: &'static str,
    pub content: &'static str,
}

macro_rules! skill {
    ($name:literal, $title:literal) => {
        Skill {
            name: $name,
            title: $title,
            content: include_str!(concat!(
                "../../../../assets/pui-engineering/skills/",
                $name,
                "/SKILL.md"
            )),
        }
    };
}

pub(super) const SKILLS: [Skill; 5] = [
    skill!("clarify", "Clarify"),
    skill!("define", "Define"),
    skill!("tdd", "TDD"),
    skill!("debug", "Debug"),
    skill!("code-review", "Code Review"),
];

impl Skill {
    pub fn description(&self) -> &str {
        self.content
            .lines()
            .find_map(|line| line.strip_prefix("description: "))
            .unwrap_or_default()
    }
}

fn root() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(macos) = exe.parent()
        && macos.file_name().is_some_and(|name| name == "MacOS")
        && macos
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "Contents")
    {
        return macos.join("../Resources/pui-engineering");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/pui-engineering")
}

pub(super) fn requests(scope: &str, selection: usize) -> Result<Vec<Value>, String> {
    requests_at(&root(), scope, selection)
}

fn requests_at(root: &Path, scope: &str, selection: usize) -> Result<Vec<Value>, String> {
    if !["user", "project"].contains(&scope) || selection > SKILLS.len() {
        return Err("Choose a valid skill and configuration scope.".into());
    }
    SKILLS
        .iter()
        .enumerate()
        .filter(|(index, _)| selection == 0 || selection == index + 1)
        .map(|(_, skill)| {
            let file = root.join("skills").join(skill.name).join("SKILL.md");
            let path = file
                .canonicalize()
                .map_err(|_| "Pui's bundled engineering skills are missing; rebuild the app.")?;
            let content = std::fs::read(&path)
                .map_err(|_| "Cannot read Pui's bundled engineering skills.")?;
            if content != skill.content.as_bytes() {
                return Err("Pui's bundled engineering skills changed; rebuild the app before enabling them.".into());
            }
            Ok(json!({"action":"register", "kind":"skills", "scope":scope,
                "source":path, "contentHash":crate::hex::lower(Sha256::digest(&content))}))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_is_opt_in_and_registers_only_selected_skills() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/pui-engineering");
        let requests = requests_at(&root, "user", 0).unwrap();
        assert_eq!(requests.len(), 5);
        for (request, skill) in requests.iter().zip(&SKILLS) {
            assert_eq!(request["action"], "register");
            assert_eq!(request["kind"], "skills");
            assert_eq!(request["scope"], "user");
            assert!(
                request["source"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("/{}/SKILL.md", skill.name))
            );
            assert_eq!(request["contentHash"].as_str().unwrap().len(), 64);
            assert!(skill.content.contains("disable-model-invocation: true"));
            assert!(!skill.description().is_empty());
        }
        let requests = requests_at(&root, "project", 3).unwrap();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0]["source"]
                .as_str()
                .unwrap()
                .ends_with("/tdd/SKILL.md")
        );
        assert_eq!(requests[0]["scope"], "project");
        assert!(requests_at(&root, "user", 6).is_err());
        assert!(requests_at(&root, "unknown", 0).is_err());
        assert!(requests_at(&root.join("missing"), "user", 0).is_err());
    }
}
