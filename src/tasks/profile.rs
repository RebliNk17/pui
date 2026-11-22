//! Versioned, repository-local execution recipe. Reading never authorizes setup
//! or execution; exact commands are copied into the native reviewed contract.
use super::*;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub recipe: plan::Recipe,
    pub steps: Vec<String>,
    pub checks: Contract,
    #[serde(default)]
    pub setup: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub digest: String,
    pub profile: Profile,
}
impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.name.trim().is_empty()
            || self.name.len() > 160
            || self.setup.len() > 8
            || self
                .setup
                .iter()
                .any(|c| c.trim().is_empty() || c.len() > 4000 || c.contains('\0'))
        {
            return Err("Invalid execution profile version, name or setup commands.".into());
        }
        super::plan::ExecutionPlan {
            steps: self.steps.clone(),
            checks: self.checks.clone(),
        }
        .validate()
    }
}
pub fn load(root: &Path) -> Result<Option<Receipt>, String> {
    // A direct filename avoids following a project-controlled .pui directory link.
    let path = root.join("pui-workflow.json");
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect workflow profile.".into()),
    };
    if !meta.is_file() || meta.len() > 64000 {
        return Err("Workflow profile must be a regular file under 64 KiB.".into());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| "Cannot safely open workflow profile.")?;
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Err("Workflow profile changed type.".into());
    }
    let mut bytes = Vec::new();
    file.take(64001)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read workflow profile.")?;
    if bytes.len() > 64000 {
        return Err("Workflow profile exceeds its bound.".into());
    }
    let profile: Profile =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid workflow profile JSON.")?;
    profile.validate()?;
    Ok(Some(Receipt {
        digest: format!("{:x}", Sha256::digest(&bytes)),
        profile,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_is_versioned_bounded_and_never_execution_authority() {
        let root = std::env::temp_dir().join(format!("pui-profile-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let data = serde_json::json!({"version":1,"name":"Verified change","steps":["Implement with coverage"],"checks":{"criteria":[{"id":"C1","description":"Tests pass","command":"npm test"}]},"setup":["npm ci --ignore-scripts"]});
        fs::write(root.join("pui-workflow.json"), data.to_string()).unwrap();
        let first = load(&root).unwrap().unwrap();
        assert_eq!(first.profile.setup.len(), 1);
        let mut changed = data;
        changed["version"] = serde_json::json!(2);
        fs::write(root.join("pui-workflow.json"), changed.to_string()).unwrap();
        assert!(load(&root).is_err());
        #[cfg(unix)]
        {
            fs::remove_file(root.join("pui-workflow.json")).unwrap();
            std::os::unix::fs::symlink("/nonexistent", root.join("pui-workflow.json")).unwrap();
            assert!(load(&root).is_err());
        }
    }
}
