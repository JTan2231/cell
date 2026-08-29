// Schema-v1 creation is no longer a production write path. The compatibility
// adapter below keeps its immutable model/tool contract covered without giving
// the legacy liaison authority over Todo v2 storage.

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::error::{AppError, AppResult};
    use crate::tool_server::{Backend, Call, LegacyCreateTodo, ToolFailure, ToolSuccess};

    #[derive(Debug, Eq, PartialEq)]
    struct LegacyTodo {
        id: &'static str,
        title: String,
        note: String,
        tool_call_id: String,
    }

    #[derive(Default)]
    struct LegacyCreationAdapter {
        created: Option<LegacyTodo>,
    }

    impl LegacyCreationAdapter {
        fn create_todo(
            &mut self,
            tool_call_id: &str,
            arguments: LegacyCreateTodo,
        ) -> Result<ToolSuccess, ToolFailure> {
            if self.created.is_some() {
                return Err(ToolFailure::new(
                    "todo_already_created",
                    "this legacy liaison session has already created its todo",
                ));
            }
            if arguments.title.trim().is_empty() {
                return Err(ToolFailure::new(
                    "invalid_title",
                    "todo title must not be blank",
                ));
            }
            if arguments.title.contains(['\n', '\r']) {
                return Err(ToolFailure::new(
                    "invalid_title",
                    "todo title must be one line",
                ));
            }
            if arguments.note.trim().is_empty() {
                return Err(ToolFailure::new(
                    "invalid_note",
                    "todo note must not be blank",
                ));
            }

            let todo = LegacyTodo {
                id: "t1",
                title: arguments.title,
                note: arguments.note,
                tool_call_id: tool_call_id.to_owned(),
            };
            let result = ToolSuccess::created(serde_json::json!({
                "created": true,
                "todo": {
                    "id": todo.id,
                    "title": todo.title,
                },
            }));
            self.created = Some(todo);
            Ok(result)
        }
    }

    impl Backend for LegacyCreationAdapter {
        fn call(&mut self, tool_call_id: &str, call: Call) -> Result<ToolSuccess, ToolFailure> {
            match call {
                Call::CreateTodo(arguments) => self.create_todo(tool_call_id, arguments),
                _ => Err(ToolFailure::new(
                    "unsupported_tool",
                    "the legacy creation adapter accepts only create_todo",
                )),
            }
        }
    }

    fn invocation_prompt(
        source: &Path,
        direction: &str,
        working_directory: &Path,
    ) -> AppResult<String> {
        let source = source.to_str().ok_or_else(|| {
            AppError::invalid(
                "invalid_source_path",
                "resolved source path must contain valid UTF-8",
            )
        })?;
        let working_directory = working_directory.to_str().ok_or_else(|| {
            AppError::invalid(
                "invalid_working_directory",
                "caller working directory must contain valid UTF-8",
            )
        })?;
        let source = serde_json::to_string(source)?;
        let direction = serde_json::to_string(direction)?;
        let working_directory = serde_json::to_string(working_directory)?;
        Ok(format!(
            "Research this need and create one actionable todo. The source is the provenance and first research lead, not the boundary of your investigation. Use the caller's working directory to identify the exact local subject before considering analogies.\n\nSource path:\n{source}\n\nCaller working directory:\n{working_directory}\n\nDirection:\n{direction}"
        ))
    }

    #[test]
    fn invocation_keeps_source_and_direction_roles_distinct()
    -> Result<(), Box<dyn std::error::Error>> {
        let prompt = invocation_prompt(
            Path::new("/tmp/source transcript.md"),
            "need to provide token consumption stats",
            Path::new("/tmp/project"),
        )?;
        assert!(prompt.contains("provenance and first research lead"));
        assert!(prompt.contains(r#""/tmp/source transcript.md""#));
        assert!(prompt.contains(r#""/tmp/project""#));
        assert!(prompt.contains(r#""need to provide token consumption stats""#));
        Ok(())
    }

    #[test]
    fn self_contained_adapter_validates_then_records_exactly_once() {
        let mut backend = LegacyCreationAdapter::default();

        let blank = backend.call(
            "legacy-call-blank",
            Call::CreateTodo(LegacyCreateTodo {
                title: " ".to_owned(),
                note: "Useful note".to_owned(),
            }),
        );
        assert_eq!(
            blank.as_ref().err().map(ToolFailure::code),
            Some("invalid_title")
        );
        assert!(backend.created.is_none());

        let Ok(created) = backend.call(
            "legacy-call-created",
            Call::CreateTodo(LegacyCreateTodo {
                title: "Report liaison token usage".to_owned(),
                note: "Actionable note".to_owned(),
            }),
        ) else {
            panic!("valid creation was rejected");
        };
        assert!(created.todo_created());
        assert_eq!(
            backend
                .created
                .as_ref()
                .map(|todo| todo.tool_call_id.as_str()),
            Some("legacy-call-created")
        );

        let duplicate = backend.call(
            "legacy-call-duplicate",
            Call::CreateTodo(LegacyCreateTodo {
                title: "Another".to_owned(),
                note: "Another note".to_owned(),
            }),
        );
        assert_eq!(
            duplicate.as_ref().err().map(ToolFailure::code),
            Some("todo_already_created")
        );
    }
}
