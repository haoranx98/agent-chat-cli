mod api;
mod chat;
mod config;

use clap::Parser;
use config::{Cli, Command};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let app_config = match config::load(&cli) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("配置错误: {error}");
            std::process::exit(1);
        }
    };

    // 省略子命令时也进入对话模式，方便直接执行 agent-chat。
    match cli.command.as_ref().unwrap_or(&Command::Chat) {
        Command::Chat => {
            if let Err(error) = chat::run(&app_config).await {
                eprintln!("错误: {error}");
                std::process::exit(1);
            }
        }
    }
}
