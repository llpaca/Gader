use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use tracing::warn;

const CONFIG_DIR: &str = ".gader";
const SECRET_FILE: &str = "server_secret";

fn get_config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("Could not find HOME environment variable")?;

    let path = PathBuf::from(home).join(CONFIG_DIR);

    if !path.exists() {
        fs::create_dir_all(&path)?;
    }

    Ok(path)
}

pub fn load_secret() -> Result<String> {
    let dir = get_config_dir()?;
    let secret_path = dir.join(SECRET_FILE);

    if secret_path.exists() {
        let secret =
            fs::read_to_string(&secret_path).context("Failed to read client secret file")?;
        Ok(secret.trim().to_string())
    } else {
        let default_secret = "change_me_please";
        fs::write(&secret_path, default_secret).context("Failed to write default secret file")?;

        warn!("No secret found. Created default at: {:?}", secret_path);
        Ok(default_secret.to_string())
    }
}
