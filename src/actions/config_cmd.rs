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
            let cfg = Config::load()?;
            match key {
                Some(key) => println!("{}", cfg.get(&key)?),
                None => {
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
