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
        ConfigAction::Get { key: Some(key) } => {
            let cfg = Config::load_file(&path)?;
            println!("{}", cfg.get(&key)?);
        }
        ConfigAction::Get { key: None } => {
            let cfg = Config::load_file(&path)?;
            print!("{}", serde_yaml::to_string(&cfg)?);
        }
        ConfigAction::Path => println!("{}", path.display()),
    }
    Ok(())
}
