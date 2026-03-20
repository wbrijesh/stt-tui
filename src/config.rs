use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Local,
    Openai,
}

impl Default for Backend {
    fn default() -> Self {
        Backend::Local
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::Local => write!(f, "local"),
            Backend::Openai => write!(f, "openai"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub backend: Backend,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_model() -> String {
    "gpt-4o-mini-transcribe".to_string()
}

impl Config {
    pub fn config_dir() -> Result<PathBuf> {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home).join(".config").join("stt-tui"))
    }

    pub fn models_dir() -> Result<PathBuf> {
        let dir = Self::config_dir()?.join("models");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut config: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", path.display()))?;

        // Env var override for API key
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            config.api_key = key;
        }

        Ok(Some(config))
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir()?;
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create {}", dir.display()))?;

        let path = Self::config_path()?;
        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&path, contents)
            .with_context(|| format!("Failed to write {}", path.display()))?;

        Ok(())
    }

    #[allow(dead_code)]
    pub fn needs_api_key(&self) -> bool {
        self.backend == Backend::Openai && self.api_key.is_empty()
    }
}
