mod agent;
mod tools;
mod wire;

use async_openai::{Client, config::OpenAIConfig};
use clap::Parser;
use std::{
    env,
    io::{self, Write},
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

    // Single-shot prompt
    if let Some(prompt) = args.prompt {
        return agent::run(&client, prompt).await;
    }

    // Multi-turn
    let mut input = String::new();
    loop {
        print!("> ");
        io::stdout().flush()?;
        input.clear();

        io::stdin().read_line(&mut input)?;
        let prompt = input.trim_end();

        if prompt == "/quit" {
            break;
        }
    }

    Ok(())
}
