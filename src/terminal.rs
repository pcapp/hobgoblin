use std::io::{self, BufRead, Write};

use crate::agent::{self, Conversation};

pub async fn run_once<M: agent::Model, W: Write>(
    client: &M,
    output: &mut W,
    prompt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut conversation = Conversation::default();
    let response = agent::turn(client, &mut conversation, prompt).await?;
    writeln!(output, "{response}")?;
    Ok(())
}

pub async fn run_interactive<M: agent::Model, R: BufRead>(
    client: &M,
    reader: &mut R,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut conversation = Conversation::default();

    let mut user_input = String::new();
    loop {
        print!("> ");
        io::stdout().flush()?;
        user_input.clear();

        let bytes_read = reader.read_line(&mut user_input)?;

        if bytes_read == 0 {
            break;
        }

        let prompt = user_input.trim();

        if prompt.is_empty() {
            continue;
        }

        if prompt == "/quit" || prompt == "/exit" {
            break;
        }

        let response = agent::turn(client, &mut conversation, prompt).await?;
        println!("{response}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Mutex};

    use serde_json::{Value, json};

    use crate::{agent, terminal::run_interactive, terminal::run_once};

    struct ScriptedModel {
        response: Value,
        requests: Mutex<Vec<Value>>,
    }

    impl ScriptedModel {
        fn new(response: Value) -> Self {
            Self {
                response,
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl agent::Model for ScriptedModel {
        async fn complete(&self, request: Value) -> Result<Value, Box<dyn std::error::Error>> {
            self.requests.lock().unwrap().push(request);
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn runs_once_writes_one_turn_answer_to_output() {
        let model = ScriptedModel::new(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "scripted answer",
                    "tool_calls": null
                }
            }]
        }));

        let mut output = Vec::new();

        run_once(&model, &mut output, "hello")
            .await
            .expect("one-shot frontend should succeed");

        assert_eq!(model.requests.lock().unwrap().len(), 1);

        assert_eq!(
            String::from_utf8(output).expect("output should be UTF-8"),
            "scripted answer\n"
        );
    }

    #[tokio::test]
    async fn run_interactive_stops_on_eof() {
        let mut input = Cursor::new(b"");
        let model = ScriptedModel::new(json!("not used"));

        let _: () = run_interactive(&model, &mut input)
            .await
            .expect("EOF should end the interactive session successfully.");

        assert_eq!(model.requests.lock().unwrap().len(), 0);
    }
}
