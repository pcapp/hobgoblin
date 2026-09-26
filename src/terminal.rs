use std::io::{self, Write};

use crate::agent::{self, Conversation};
use async_openai::{Client, config::OpenAIConfig};

pub async fn run_once(
    client: &Client<OpenAIConfig>,
    prompt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut conversation = Conversation::default();
    let response = agent::turn(client, &mut conversation, prompt).await?;
    println!("{response}");
    Ok(())
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

        let response = agent::turn(client, &mut conversation, prompt).await?;
        println!("{response}");
    }

    Ok(())
}
