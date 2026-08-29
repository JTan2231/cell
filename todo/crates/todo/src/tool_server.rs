use serde_json::{Value, json};

#[path = "stage_contracts.rs"]
pub(crate) mod contracts;

#[cfg(test)]
pub(crate) use contracts::LegacyCreateTodo;
pub(crate) use contracts::{
    Backend, Call, ManagedTool, Stage, StageContract, WorkspacePolicy, contract,
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolFailure {
    code: String,
    message: String,
}

impl ToolFailure {
    #[must_use]
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolSuccess {
    output: Value,
    todo_created: bool,
}

impl ToolSuccess {
    /// Preserve the historical v1 `create_todo` result envelope.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn created(output: Value) -> Self {
        Self {
            output,
            todo_created: true,
        }
    }

    /// Return bounded read data to a v2 liaison.
    #[must_use]
    #[allow(dead_code)] // Used by the v2 store backends wired in the companion change.
    pub(crate) fn data(data: Value) -> Self {
        let mut output = serde_json::Map::new();
        output.insert("ok".to_owned(), Value::Bool(true));
        output.insert("data".to_owned(), data);
        Self {
            output: Value::Object(output),
            todo_created: false,
        }
    }

    /// Return a Todo-owned proposal, assessment, or draft identity.
    #[must_use]
    #[allow(dead_code)] // Used by the v2 store backends wired in the companion change.
    pub(crate) fn recorded(kind: &str, id: &str, version: u64, status: &str) -> Self {
        Self {
            output: json!({
                "ok": true,
            "artifact": {
                "kind": kind,
                "id": id,
                "version": version,
                "status": status,
            }
            }),
            todo_created: false,
        }
    }

    #[must_use]
    pub(crate) fn output(&self) -> &Value {
        &self.output
    }

    #[must_use]
    #[allow(dead_code)] // Historical compatibility assertion.
    pub(crate) const fn todo_created(&self) -> bool {
        self.todo_created
    }
}

pub(crate) fn dispatch(
    backend: &mut impl Backend,
    contract: &StageContract,
    tool_call_id: &str,
    name: &str,
    arguments: Value,
) -> Result<ToolSuccess, ToolFailure> {
    let Some(definition) = contract.tool_named(name) else {
        return Err(ToolFailure::new(
            "unknown_tool",
            format!(
                "tool {name:?} is not part of the {} contract",
                contract.toolset_name
            ),
        ));
    };
    let call = contracts::decode_call(definition.tool, arguments)?;
    backend.call(tool_call_id, call)
}

// Historical v1 accessors remain byte-compatible for the existing `todo new`
// requester while v2 callers select an explicit Stage contract.
#[allow(dead_code)]
pub(crate) fn instructions() -> &'static str {
    contracts::LEGACY_INSTRUCTIONS
}

#[allow(dead_code)]
pub(crate) fn tool_definitions() -> Vec<Value> {
    contract(Stage::LegacyCreation)
        .tools
        .iter()
        .map(ManagedTool::definition)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{Backend, Call, Stage, ToolFailure, ToolSuccess, contract, dispatch, instructions};

    #[derive(Default)]
    struct StubBackend {
        calls: Vec<(String, Call)>,
    }

    impl Backend for StubBackend {
        fn call(&mut self, tool_call_id: &str, call: Call) -> Result<ToolSuccess, ToolFailure> {
            self.calls.push((tool_call_id.to_owned(), call));
            Ok(ToolSuccess::data(json!({ "accepted": true })))
        }
    }

    #[test]
    fn legacy_contract_still_exposes_only_create_todo() {
        let contract = contract(Stage::LegacyCreation);
        assert_eq!(contract.tools.len(), 1);
        assert_eq!(contract.tools[0].tool.name(), "create_todo");
        assert!(contract.tool_named("write_file").is_none());
    }

    #[test]
    fn stage_dispatch_rejects_cross_contract_tools_before_the_backend() {
        let contract = contract(Stage::ConcernRouting);
        let mut backend = StubBackend::default();
        let result = dispatch(
            &mut backend,
            &contract,
            "call-cross-contract",
            "submit_situation_assessment",
            Value::Object(serde_json::Map::new()),
        );
        assert_eq!(
            result.as_ref().err().map(ToolFailure::code),
            Some("unknown_tool")
        );
        assert!(backend.calls.is_empty());
    }

    #[test]
    fn stage_dispatch_forwards_the_managed_tool_call_id() {
        let contract = contract(Stage::ConcernRouting);
        let mut backend = StubBackend::default();
        let result = dispatch(
            &mut backend,
            &contract,
            "call-routing-source-7",
            "routing_source_overview",
            json!({}),
        );
        assert!(result.is_ok());
        assert_eq!(backend.calls.len(), 1);
        assert_eq!(backend.calls[0].0, "call-routing-source-7");
        assert!(matches!(
            &backend.calls[0].1,
            Call::RoutingSourceOverview(_)
        ));
    }

    #[test]
    fn legacy_prompt_retains_historical_research_contract() {
        let prompt = instructions();
        assert!(prompt.contains("source is the beginning of the investigation"));
        assert!(prompt.contains("history_base"));
        assert!(prompt.contains("call create_todo exactly once"));
    }
}
