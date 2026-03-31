use opencli_rs_core::CliError;
use std::path::PathBuf;

use crate::types::ExternalCli;

/// Embedded default external CLIs from resources/external-clis.yaml
const BUILTIN_EXTERNAL_CLIS: &str = include_str!("../resources/external-clis.yaml");

/// Return the path to the user's external-clis.yaml override file.
fn user_external_clis_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".opencli-rs")
        .join("external-clis.yaml")
}

/// Load external CLI definitions from the embedded resource and optionally
/// from the user's `~/.opencli-rs/external-clis.yaml`.
///
/// User definitions are merged on top: if a user defines a CLI with the same
/// `name` as a builtin one, the user version wins.
pub fn load_external_clis() -> Result<Vec<ExternalCli>, CliError> {
    let mut clis: Vec<ExternalCli> = serde_yaml::from_str(BUILTIN_EXTERNAL_CLIS)?;

    let user_path = user_external_clis_path();
    if user_path.exists() {
        match std::fs::read_to_string(&user_path) {
            Ok(content) => match serde_yaml::from_str::<Vec<ExternalCli>>(&content) {
                Ok(user_clis) => {
                    for ucli in user_clis {
                        // Replace existing by name, or append
                        if let Some(pos) = clis.iter().position(|c| c.name == ucli.name) {
                            clis[pos] = ucli;
                        } else {
                            clis.push(ucli);
                        }
                    }
                    tracing::debug!(path = ?user_path, "Loaded user external CLIs");
                }
                Err(e) => {
                    tracing::warn!(path = ?user_path, error = %e, "Failed to parse user external-clis.yaml");
                }
            },
            Err(e) => {
                tracing::warn!(path = ?user_path, error = %e, "Failed to read user external-clis.yaml");
            }
        }
    }

    Ok(clis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_builtin_external_clis() {
        let clis = load_external_clis().unwrap();
        assert!(!clis.is_empty());
        // gh should be present in builtins
        assert!(clis.iter().any(|c| c.name == "gh"));
        assert!(clis.iter().any(|c| c.name == "docker"));
    }

    #[test]
    fn test_builtin_yaml_parses() {
        let clis: Vec<ExternalCli> = serde_yaml::from_str(BUILTIN_EXTERNAL_CLIS).unwrap();
        assert!(clis.len() >= 6);
        let gh = clis.iter().find(|c| c.name == "gh").unwrap();
        assert_eq!(gh.binary, "gh");
        assert!(!gh.tags.is_empty());
    }
}
