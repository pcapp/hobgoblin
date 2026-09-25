use std::io::{self, Write};

use crate::agent::{self, Conversation};
use async_openai::{Client, config::OpenAIConfig};

pub async fn run_once(
    client: &Client<OpenAIConfig>,
    prompt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut conversation = Conversation::default();
    agent::turn(&client, &mut conversation, prompt).await
}

pub async fn run_interactive(
    client: &Client<OpenAIConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut conversation = Conversation::default();

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

        agent::turn(&client, &mut conversation, prompt).await?;
    }

    Ok(())
}
