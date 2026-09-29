mod agent;
mod terminal;
mod tools;
mod wire;

use async_openai::{Client, config::OpenAIConfig};
use clap::Parser;
use std::{
    env,
    io::{stdin, stdout},
    process,
};

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    #[arg(short = 'p', long)]
    prompt: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    dotenvy::dotenv().ok();

    let base_url = env::var("OPENROUTER_BASE_URL")
        .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());

    let api_key = env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| {
        eprintln!("OPENROUTER_API_KEY is not set");
        process::exit(1);
    });

    let config = OpenAIConfig::new()
        .with_api_base(base_url)
        .with_api_key(api_key);

    let client = Client::with_config(config);

    return match args.prompt {
        Some(prompt) => {
            let mut stdout = std::io::stdout();
            terminal::run_once(&client, &mut stdout, prompt.as_str()).await
        }
        None => {
            let mut reader = stdin().lock();
            let mut writer = stdout().lock();
            terminal::run_interactive(&client, &mut reader, &mut writer).await
        }
    };
}
