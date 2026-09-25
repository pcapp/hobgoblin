use std::io::{self, Write};

use crate::agent;
use async_openai::{Client, config::OpenAIConfig};

pub async fn run_once(
    client: &Client<OpenAIConfig>,
    prompt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    agent::run(&client, prompt).await
}

pub async fn run_interactive(
    _client: &Client<OpenAIConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
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
