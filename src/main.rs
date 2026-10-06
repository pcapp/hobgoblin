mod acp;
mod agent;
mod skills;
mod terminal;
mod tools;
mod wire;

use async_openai::{Client, config::OpenAIConfig};
use clap::Parser;
use std::{
    env,
    io::{stdin, stdout},
    path::PathBuf,
    process,
};

use crate::skills::FileSystemSkillLoader;

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    #[arg(short = 'p', long)]
    prompt: Option<String>,

    #[arg(long, conflicts_with = "prompt")]
    acp: bool,
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

    if args.acp {
        let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
        return acp::run_acp(&mut reader).await;
    }

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

    let skill_loader = FileSystemSkillLoader {
        skill_root: PathBuf::from(".claude/skills"),
    };

    if let Some(prompt) = args.prompt {
        let mut stdout = std::io::stdout();
        return terminal::run_once(&client, &skill_loader, &mut stdout, prompt.as_str()).await;
    };

    let mut reader = stdin().lock();
    let mut writer = stdout().lock();
    terminal::run_interactive(&client, &skill_loader, &mut reader, &mut writer).await
}

#[cfg(test)]
mod tests {
    use super::Args;
    use clap::Parser;

    #[test]
    fn no_arguments_selects_interactive_mode() {
        let args = Args::try_parse_from(["hobgoblin"]).expect("no arguments should be accepted");

        assert_eq!(args.prompt, None);
        assert!(!args.acp);
    }

    #[test]
    fn rejects_prompt_argument_without_a_value() {
        let result = Args::try_parse_from(["hobgoblin", "-p"]);

        assert!(result.is_err());
    }

    #[test]
    fn prompt_argument_selects_one_shot_mode() {
        let args = Args::try_parse_from(["hobgoblin", "-p", "hello"])
            .expect("-p with a value should be accepted");

        assert_eq!(args.prompt, Some(String::from("hello")));
        assert!(!args.acp);
    }

    #[test]
    fn accepts_reserved_acp_argument() {
        let args = Args::try_parse_from(["hobgoblin", "--acp"]).expect("--acp should be accepted");

        assert_eq!(args.prompt, None);
        assert!(args.acp);
    }

    #[test]
    fn rejects_invocations_with_both_acp_and_one_shot_options() {
        let result = Args::try_parse_from(["hobgoblin", "--acp", "-p", "What is 1 + 1?"]);

        assert!(result.is_err());
    }
}
