use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(
    name = "agent-chat",
    version,
    about = "与 OpenAI 兼容的 Agent 服务进行命令行对话"
)]
pub struct Cli {
    /// YAML 配置文件路径
    #[arg(short = 'c', long, value_name = "FILE", global = true)]
    pub config: Option<PathBuf>,

    /// 服务端 IP 或主机名，例如 127.0.0.1
    #[arg(short = 'i', long, global = true)]
    pub ip: Option<String>,

    /// 服务端端口
    #[arg(short, long, global = true)]
    pub port: Option<u16>,

    /// 请求使用的模型名称
    #[arg(short, long, global = true)]
    pub model: Option<String>,

    /// 请求超时时间（秒）
    #[arg(long, global = true)]
    pub timeout: Option<u64>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 开启交互式对话模式
    Chat,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileConfig {
    ip: Option<String>,
    port: Option<u16>,
    model: Option<String>,
    timeout: Option<u64>,
}

#[derive(Debug)]
pub struct AppConfig {
    pub ip: String,
    pub port: u16,
    pub model: String,
    pub timeout: u64,
}

impl FileConfig {
    fn defaults() -> Self {
        Self {
            ip: Some("127.0.0.1".to_owned()),
            port: Some(8031),
            model: Some("Qwen2.5-0.5B-Instruct".to_owned()),
            timeout: Some(300),
        }
    }

    fn fill_defaults(&mut self) {
        let defaults = Self::defaults();
        if self.ip.is_none() {
            self.ip = defaults.ip;
        }
        if self.port.is_none() {
            self.port = defaults.port;
        }
        if self.model.is_none() {
            self.model = defaults.model;
        }
        if self.timeout.is_none() {
            self.timeout = defaults.timeout;
        }
    }

    fn apply_cli(&mut self, cli: &Cli) {
        if let Some(ip) = &cli.ip {
            self.ip = Some(ip.clone());
        }
        if let Some(port) = cli.port {
            self.port = Some(port);
        }
        if let Some(model) = &cli.model {
            self.model = Some(model.clone());
        }
        if let Some(timeout) = cli.timeout {
            self.timeout = Some(timeout);
        }
    }

    fn into_app_config(mut self) -> AppConfig {
        self.fill_defaults();
        AppConfig {
            ip: self.ip.expect("default ip must exist"),
            port: self.port.expect("default port must exist"),
            model: self.model.expect("default model must exist"),
            timeout: self.timeout.expect("default timeout must exist"),
        }
    }
}

impl Cli {
    fn has_runtime_overrides(&self) -> bool {
        self.ip.is_some() || self.port.is_some() || self.model.is_some() || self.timeout.is_some()
    }
}

pub fn load(cli: &Cli) -> Result<AppConfig, Box<dyn std::error::Error>> {
    let path = config_path(cli)?;
    let exists = path.exists();
    let mut file_config = if exists {
        load_file(&path)?
    } else {
        FileConfig::default()
    };

    if cli.has_runtime_overrides() || !exists {
        file_config.apply_cli(cli);
        file_config.fill_defaults();
        save_file(&path, &file_config)?;
        println!(
            "配置文件已{}: {}",
            if exists { "更新" } else { "生成" },
            path.display()
        );
    }

    Ok(file_config.into_app_config())
}

fn config_path(cli: &Cli) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = &cli.config {
        return Ok(path.clone());
    }

    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or_else(|| "无法确定可执行文件所在目录".to_owned())?;
    Ok(directory.join("config.yaml"))
}

fn load_file(path: &Path) -> Result<FileConfig, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("无法读取配置文件 {}: {error}", path.display()))?;
    serde_yaml::from_str(&content)
        .map_err(|error| format!("无法解析 YAML 配置文件 {}: {error}", path.display()).into())
}

fn save_file(path: &Path, config: &FileConfig) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建配置文件目录 {}: {error}", parent.display()))?;
    }
    let content = serde_yaml::to_string(config)
        .map_err(|error| format!("无法生成 YAML 配置文件 {}: {error}", path.display()))?;
    std::fs::write(path, content)
        .map_err(|error| format!("无法写入配置文件 {}: {error}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_values_override_yaml_and_defaults_fill_missing_values() {
        let mut config = FileConfig {
            ip: Some("192.168.1.10".into()),
            port: Some(9000),
            model: Some("test-model".into()),
            timeout: None,
        };
        let cli = Cli {
            config: None,
            ip: Some("10.0.0.2".into()),
            port: None,
            model: None,
            timeout: Some(10),
            command: None,
        };

        config.apply_cli(&cli);
        let config = config.into_app_config();
        assert_eq!(config.ip, "10.0.0.2");
        assert_eq!(config.port, 9000);
        assert_eq!(config.model, "test-model");
        assert_eq!(config.timeout, 10);
    }
}
