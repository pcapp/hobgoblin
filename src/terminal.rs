use std::io::{BufRead, Error, Write};

use serde_json::json;

use crate::{
    agent::{self},
    session::Session,
    skills::SkillLoader,
};

fn create_new_session<L: SkillLoader>(skill_loader: &L) -> Result<Session, Error> {
    let mut session = Session::new(skill_loader)?;

    let skills_by_name = skill_loader.load_skills()?;

    let skill_message = format!(
        "You have access to the following skills:\n\n{}",
        skills_by_name
            .iter()
            .map(|(name, skill)| format!("- {}: {}", name, skill.description))
            .collect::<Vec<String>>()
            .join("\n")
    );

    if !skills_by_name.is_empty() {
        session.conversation.messages.push(json!({
            "role": "system",
            "content": skill_message
        }));
    }

    Ok(session)
}

pub async fn run_once<M: agent::Model, W: Write, L: SkillLoader>(
    client: &M,
    skill_loader: &L,
    output: &mut W,
    prompt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut conversation = create_new_session(skill_loader)?;

    let response = agent::turn(client, &mut conversation, prompt).await?;
    writeln!(output, "{response}")?;
    Ok(())
}

const PROMPT: &str = "> ";

pub async fn run_interactive<M: agent::Model, R: BufRead, W: Write, L: SkillLoader>(
    client: &M,
    skill_loader: &L,
    reader: &mut R,
    writer: &mut W,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut conversation = create_new_session(skill_loader)?;

    let mut user_input = String::new();
    loop {
        write!(writer, "{PROMPT}")?;
        writer.flush()?;
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
        writeln!(writer, "{response}")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashMap},
        io::Cursor,
        sync::Mutex,
    };

    use serde_json::{Value, json};

    use crate::{
        agent,
        skills::SkillLoader,
        terminal::{run_interactive, run_once},
    };

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

    struct EmptySkillLoader {}

    impl SkillLoader for EmptySkillLoader {
        fn load_skills(
            &self,
        ) -> Result<std::collections::BTreeMap<String, crate::skills::Skill>, std::io::Error>
        {
            Ok(BTreeMap::new())
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

        let skill_loader = EmptySkillLoader {};

        let mut output = Vec::new();

        run_once(&model, &skill_loader, &mut output, "hello")
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
        let skill_loader = EmptySkillLoader {};
        let mut output = Vec::new();

        let _: () = run_interactive(&model, &skill_loader, &mut input, &mut output)
            .await
            .expect("EOF should end the interactive session successfully.");

        assert_eq!(String::from_utf8(output).unwrap(), "> ");

        assert_eq!(model.requests.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn interactive_reuses_conversation_across_prompts() {
        let model = ScriptedModel::new(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "scripted answer",
                    "tool_calls": null
                }
            }]
        }));

        let skill_loader = EmptySkillLoader {};

        let mut output = Vec::new();
        let mut input = Cursor::new(b"Hello.\nGoodbye.\n");

        run_interactive(&model, &skill_loader, &mut input, &mut output)
            .await
            .expect("interactive session should complete");

        let requests = model.requests.lock().unwrap();

        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1]["messages"],
            json!([
                {
                    "role": "user",
                    "content": "Hello."
                },
                {
                    "role": "assistant",
                    "content": "scripted answer",
                    "tool_calls": null
                },
                {
                    "role": "user",
                    "content": "Goodbye."
                }
            ])
        );

        assert_eq!(
            String::from_utf8(output).expect("output should be UTF-8"),
            "> scripted answer\n> scripted answer\n> "
        );
    }

    #[tokio::test]
    async fn interactive_exit_command_does_not_start_another_turn() {
        let model = ScriptedModel::new(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "scripted answer",
                    "tool_calls": null
                }
            }]
        }));

        let skill_loader = EmptySkillLoader {};

        let mut input = Cursor::new(b"Hello.\n/exit\n");
        let mut output = Vec::new();

        run_interactive(&model, &skill_loader, &mut input, &mut output)
            .await
            .expect("/exit should end the interactive session successfully");

        assert_eq!(model.requests.lock().unwrap().len(), 1);
        assert_eq!(
            String::from_utf8(output).expect("output should be UTF-8"),
            "> scripted answer\n> "
        );
    }

    #[tokio::test]
    async fn interactive_quit_command_does_not_start_another_turn() {
        let model = ScriptedModel::new(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "scripted answer",
                    "tool_calls": null
                }
            }]
        }));

        let skill_loader = EmptySkillLoader {};

        let mut input = Cursor::new(b"Hello.\n/quit\n");
        let mut output = Vec::new();

        run_interactive(&model, &skill_loader, &mut input, &mut output)
            .await
            .expect("/quit should end the interactive session successfully");

        assert_eq!(model.requests.lock().unwrap().len(), 1);
        assert_eq!(
            String::from_utf8(output).expect("output should be UTF-8"),
            "> scripted answer\n> "
        );
    }
}
