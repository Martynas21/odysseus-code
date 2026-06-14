use anyhow::Result;

use crate::cli::ConfigAction;
use crate::config::{Config, config_path};

pub fn handle(action: ConfigAction) -> Result<()> {
    let path = config_path()?;
    match action {
        ConfigAction::Set { key, value } => {
            let mut cfg = Config::load_file(&path)?;
            cfg.set(&key, &value)?;
            cfg.save_to(&path)?;
            println!("{key} = {} ({})", cfg.get(&key)?, path.display());
        }
        ConfigAction::Get { key } => {
            // Reflect the effective config (with env overrides), matching what
            // real commands actually use.
            let cfg = Config::load()?;
            match key {
                Some(key) => println!("{}", cfg.get(&key)?),
                None => {
                    // Don't echo the secret token in the whole-config dump; it's
                    // incidental output the user didn't ask for by name (and may
                    // be an env-injected value never stored on disk). Explicit
                    // `config get api_key` still returns the real value.
                    let mut redacted = cfg;
                    if !redacted.api_key.is_empty() {
                        redacted.api_key = "***".into();
                    }
                    print!("{}", serde_yaml::to_string(&redacted)?);
                }
            }
        }
        ConfigAction::Path => println!("{}", path.display()),
    }
    Ok(())
}
