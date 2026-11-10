//! Explicit CLI proposal request. Never approves or starts implementation.
use super::Task;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub source: PathBuf,
    #[serde(default)]
    pub parent_session: Option<PathBuf>,
    pub provider: String,
    pub model: String,
    pub thinking: String,
    pub objective: String,
    pub constraints: String,
    pub spend_limit: f64,
}
impl Request {
    pub fn load(path: &Path) -> Result<Self, String> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.len() > 64000 {
            return Err("Proposal request must be a regular JSON file under 64 KiB.".into());
        }
        let request: Self =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|_| "Invalid proposal request")?;
        if !request.spend_limit.is_finite()
            || request.spend_limit <= 0.0
            || request.spend_limit > 1000.0
        {
            return Err("Proposal spend threshold must be between $0 and $1000.".into());
        }
        Ok(request)
    }
    pub fn task(self) -> Result<Task, String> {
        let source = std::fs::canonicalize(&self.source).map_err(|e| e.to_string())?;
        if !source.is_dir() {
            return Err("Proposal source is not a directory.".into());
        }
        let mut t = Task::new(source, self.provider, self.model, self.thinking);
        t.parent_session = self.parent_session;
        t.objective = self.objective;
        t.constraints = self.constraints;
        t.dynamic_routing = true;
        t.spend_limit = Some(self.spend_limit);
        t.validate_draft()?;
        Ok(t)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_file_is_bounded_and_regular() {
        let root =
            std::env::temp_dir().join(format!("pui-proposal-request-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("request.json");
        std::fs::write(&path,serde_json::to_vec(&serde_json::json!({"source":root,"provider":"p","model":"m","thinking":"high","objective":"Inspect layout","constraints":"read only","spend_limit":10.0})).unwrap()).unwrap();
        assert!(Request::load(&path).is_ok());
        assert!(Request::load(&root).is_err());
        std::fs::write(&path, vec![b' '; 64001]).unwrap();
        assert!(Request::load(&path).is_err());
    }
    #[test]
    fn request_cannot_grant_execution_or_inject_saved_state() {
        let data = serde_json::json!({"source":"/tmp","provider":"p","model":"m","thinking":"high","objective":"Inspect layout","constraints":"Read only","spend_limit":10.0});
        let r: Request = serde_json::from_value(data.clone()).unwrap();
        let t = r.task().unwrap();
        assert!(t.approved.is_none());
        assert_eq!(t.phase, super::super::Phase::Draft);
        assert!(t.dynamic_routing);
        assert!(t.runs.is_empty());
        let mut linked = data.clone();
        linked["parent_session"] = serde_json::json!("/fixture/parent.jsonl");
        let linked: Request = serde_json::from_value(linked).unwrap();
        assert_eq!(
            linked.task().unwrap().parent_session.as_deref(),
            Some(Path::new("/fixture/parent.jsonl"))
        );
        let mut bad = data;
        bad["approved"] = serde_json::json!({"criteria":[]});
        assert!(serde_json::from_value::<Request>(bad).is_err());
    }
}
