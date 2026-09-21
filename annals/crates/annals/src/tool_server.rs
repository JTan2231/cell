use serde_json::{Value, json};

const INSTRUCTIONS: &str = "<bazaar:annals.liaison.instructions>";

pub(crate) const fn instructions() -> &'static str {
    INSTRUCTIONS
}

/// A model-facing operation exposed by one liaison session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    WorkOverview,
    WorkRead,
    WorkSearch,
    CorpusSearch,
    CorpusInspect,
    SubmitReconciliation,
    ReviseReconciliation,
    ReconciliationStatus,
    DiscardReconciliation,
}

impl Tool {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "work_overview" => Some(Self::WorkOverview),
            "work_read" => Some(Self::WorkRead),
            "work_search" => Some(Self::WorkSearch),
            "corpus_search" => Some(Self::CorpusSearch),
            "corpus_inspect" => Some(Self::CorpusInspect),
            "submit_reconciliation" => Some(Self::SubmitReconciliation),
            "revise_reconciliation" => Some(Self::ReviseReconciliation),
            "reconciliation_status" => Some(Self::ReconciliationStatus),
            "discard_reconciliation" => Some(Self::DiscardReconciliation),
            _ => None,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::WorkOverview => "work_overview",
            Self::WorkRead => "work_read",
            Self::WorkSearch => "work_search",
            Self::CorpusSearch => "corpus_search",
            Self::CorpusInspect => "corpus_inspect",
            Self::SubmitReconciliation => "submit_reconciliation",
            Self::ReviseReconciliation => "revise_reconciliation",
            Self::ReconciliationStatus => "reconciliation_status",
            Self::DiscardReconciliation => "discard_reconciliation",
        }
    }

    pub(crate) const fn mutates_reconciliation_draft(self) -> bool {
        matches!(
            self,
            Self::SubmitReconciliation | Self::ReviseReconciliation | Self::DiscardReconciliation
        )
    }
}

/// A recoverable tool failure returned to the model as a normal tool result.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolFailure {
    code: String,
    message: String,
    details: Option<Value>,
}

impl ToolFailure {
    #[must_use]
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    #[must_use]
    pub(crate) fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    #[must_use]
    pub(crate) fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub(crate) fn details(&self) -> Option<&Value> {
        self.details.as_ref()
    }
}

/// Application logic behind the nine session-scoped liaison tools.
///
/// The tool interface exposes durable public concept IDs, but no private database details. A
/// concrete backend is created with one work and one base revision already bound to it, and
/// receives only the language-level arguments supplied by the model.
pub(crate) struct ToolSuccess {
    output: Value,
    reconciliation_recorded: bool,
}

impl ToolSuccess {
    #[must_use]
    pub(crate) fn new(output: Value) -> Self {
        Self {
            output,
            reconciliation_recorded: false,
        }
    }

    #[must_use]
    pub(crate) fn recorded(output: Value) -> Self {
        Self {
            output,
            reconciliation_recorded: true,
        }
    }

    #[must_use]
    pub(crate) fn output(&self) -> &Value {
        &self.output
    }

    #[must_use]
    pub(crate) const fn reconciliation_recorded(&self) -> bool {
        self.reconciliation_recorded
    }
}

pub(crate) trait Backend {
    fn call(&mut self, tool: Tool, arguments: Value) -> Result<ToolSuccess, ToolFailure>;
}

#[allow(clippy::too_many_lines)]
pub(crate) fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "work_overview",
            "description": "<bazaar:annals.tools.work_overview.description>",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {}
            }
        }),
        json!({
            "name": "work_read",
            "description": "<bazaar:annals.tools.work_read.description>",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["regions"],
                "properties": {
                    "regions": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 20,
                        "items": {
                            "type": "object",
                            "description": "<bazaar:annals.tools.work_read.region.description>",
                            "additionalProperties": false,
                            "properties": {
                                "heading_path": {
                                    "type": "array",
                                    "minItems": 1,
                                    "items": { "type": "string", "minLength": 1 },
                                    "description": "<bazaar:annals.tools.work_read.heading_path.description>"
                                },
                                "around_quote": {
                                    "type": "string",
                                    "minLength": 1,
                                    "description": "<bazaar:annals.tools.work_read.around_quote.description>"
                                },
                                "after_quote": {
                                    "type": "string",
                                    "minLength": 1,
                                    "description": "<bazaar:annals.tools.work_read.after_quote.description>"
                                },
                                "edge": { "type": "string", "enum": ["beginning", "end"] },
                                "max_characters": {
                                    "type": "integer",
                                    "minimum": 1,
                                    "maximum": 12000,
                                    "default": 4000
                                }
                            },
                            "oneOf": [
                                { "required": ["heading_path"] },
                                { "required": ["around_quote"] },
                                { "required": ["after_quote"] },
                                { "required": ["edge"] }
                            ]
                        }
                    }
                }
            }
        }),
        json!({
            "name": "work_search",
            "description": "<bazaar:annals.tools.work_search.description>",
            "inputSchema": work_search_schema()
        }),
        json!({
            "name": "corpus_search",
            "description": "<bazaar:annals.tools.corpus_search.description>",
            "inputSchema": corpus_search_schema()
        }),
        json!({
            "name": "corpus_inspect",
            "description": "<bazaar:annals.tools.corpus_inspect.description>",
            "inputSchema": corpus_inspect_schema()
        }),
        json!({
            "name": "submit_reconciliation",
            "description": "<bazaar:annals.tools.submit_reconciliation.description>",
            "inputSchema": submit_reconciliation_schema()
        }),
        json!({
            "name": "revise_reconciliation",
            "description": "<bazaar:annals.tools.revise_reconciliation.description>",
            "inputSchema": revise_reconciliation_schema()
        }),
        json!({
            "name": "reconciliation_status",
            "description": "<bazaar:annals.tools.reconciliation_status.description>",
            "inputSchema": reconciliation_status_schema()
        }),
        json!({
            "name": "discard_reconciliation",
            "description": "<bazaar:annals.tools.discard_reconciliation.description>",
            "inputSchema": discard_reconciliation_schema()
        }),
    ]
}

fn concept_id_schema() -> Value {
    json!({
        "type": "string",
        "pattern": "^c[1-9][0-9]*$",
        "description": "<bazaar:annals.schema.concept_id.description>"
    })
}

fn work_search_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["queries"],
        "properties": {
            "queries": {
                "type": "array",
                "minItems": 1,
                "maxItems": 20,
                "items": { "type": "string", "minLength": 1 }
            },
            "max_results_per_query": {
                "type": "integer",
                "minimum": 1,
                "maximum": 10,
                "default": 5
            }
        }
    })
}

fn corpus_search_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["queries"],
        "properties": {
            "queries": {
                "type": "array",
                "minItems": 1,
                "maxItems": 20,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["query"],
                    "properties": {
                        "query": { "type": "string", "minLength": 1 },
                        "within": {
                            "type": "string",
                            "pattern": "^c[1-9][0-9]*$",
                            "description": "<bazaar:annals.schema.corpus_search.ancestor.description>"
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 50,
                            "default": 10
                        },
                        "cursor": {
                            "type": "string",
                            "minLength": 1,
                            "description": "<bazaar:annals.schema.corpus_search.cursor.description>"
                        }
                    }
                }
            }
        }
    })
}

#[allow(clippy::too_many_lines)]
fn corpus_inspect_schema() -> Value {
    let id = concept_id_schema();
    let limit = json!({
        "type": "integer",
        "minimum": 1,
        "maximum": 100,
        "default": 25
    });
    let cursor = json!({
        "type": "string",
        "minLength": 1,
        "description": "<bazaar:annals.schema.corpus_inspect.cursor.description>"
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["requests"],
        "properties": {
            "requests": {
                "type": "array",
                "minItems": 1,
                "maxItems": 20,
                "items": {
                    "oneOf": [
                        {
                            "type": "object",
                            "description": "<bazaar:annals.schema.corpus_inspect.overview.description>",
                            "additionalProperties": false,
                            "required": ["kind"],
                            "properties": { "kind": { "const": "overview" } }
                        },
                        {
                            "type": "object",
                            "description": "<bazaar:annals.schema.corpus_inspect.roots.description>",
                            "additionalProperties": false,
                            "required": ["kind"],
                            "properties": {
                                "kind": { "const": "roots" },
                                "limit": limit,
                                "cursor": cursor
                            }
                        },
                        {
                            "type": "object",
                            "description": "<bazaar:annals.schema.corpus_inspect.concept.description>",
                            "additionalProperties": false,
                            "required": ["kind", "id"],
                            "properties": {
                                "kind": { "const": "concept" },
                                "id": id,
                                "preview_limit": {
                                    "type": "integer",
                                    "minimum": 0,
                                    "maximum": 20,
                                    "default": 5
                                }
                            }
                        },
                        {
                            "type": "object",
                            "description": "<bazaar:annals.schema.corpus_inspect.parents.description>",
                            "additionalProperties": false,
                            "required": ["kind", "id"],
                            "properties": {
                                "kind": { "const": "parents" },
                                "id": id,
                                "limit": limit,
                                "cursor": cursor
                            }
                        },
                        {
                            "type": "object",
                            "description": "<bazaar:annals.schema.corpus_inspect.children.description>",
                            "additionalProperties": false,
                            "required": ["kind", "id"],
                            "properties": {
                                "kind": { "const": "children" },
                                "id": id,
                                "limit": limit,
                                "cursor": cursor
                            }
                        },
                        {
                            "type": "object",
                            "description": "<bazaar:annals.schema.corpus_inspect.evidence.description>",
                            "additionalProperties": false,
                            "required": ["kind", "id"],
                            "properties": {
                                "kind": { "const": "evidence" },
                                "id": id,
                                "limit": limit,
                                "cursor": cursor
                            }
                        },
                        {
                            "type": "object",
                            "description": "<bazaar:annals.schema.corpus_inspect.expand.description>",
                            "additionalProperties": false,
                            "required": ["kind", "id"],
                            "properties": {
                                "kind": { "const": "graph" },
                                "id": id,
                                "direction": {
                                    "type": "string",
                                    "enum": ["parents", "children", "both"],
                                    "default": "children"
                                },
                                "depth": {
                                    "type": "integer",
                                    "minimum": 0,
                                    "maximum": 5,
                                    "default": 2
                                },
                                "max_nodes": {
                                    "type": "integer",
                                    "minimum": 1,
                                    "maximum": 500,
                                    "default": 100
                                }
                            }
                        }
                    ]
                }
            }
        }
    })
}

#[allow(clippy::too_many_lines)]
fn submit_reconciliation_schema() -> Value {
    let concept = json!({
        "description": "<bazaar:annals.schema.concept_selector.description>",
        "oneOf": [
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": { "id": concept_id_schema() }
            },
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["new"],
                "properties": { "new": {
                    "type": "string",
                    "minLength": 1,
                    "description": "<bazaar:annals.schema.concept_selector.new.description>"
                } }
            }
        ]
    });
    let evidence = json!({
        "type": "object",
        "description": "<bazaar:annals.schema.evidence_selector.description>",
        "additionalProperties": false,
        "required": ["quote"],
        "properties": {
            "quote": { "type": "string", "minLength": 1, "description": "<bazaar:annals.schema.evidence_selector.quote.description>" },
            "within_heading": {
                "type": "array",
                "minItems": 1,
                "items": { "type": "string", "minLength": 1 },
                "description": "<bazaar:annals.schema.evidence_selector.heading_path.description>"
            },
            "preceded_by": { "type": "string", "minLength": 1, "description": "<bazaar:annals.schema.evidence_selector.preceded_by.description>" },
            "followed_by": { "type": "string", "minLength": 1, "description": "<bazaar:annals.schema.evidence_selector.followed_by.description>" }
        }
    });
    let evidence_list = json!({
        "type": "array",
        "minItems": 1,
        "items": evidence
    });
    let operations = json!({
        "type": "array",
        "minItems": 1,
        "items": {
            "oneOf": [
                {
                    "type": "object",
                    "description": "<bazaar:annals.schema.create_concept.description>",
                    "additionalProperties": false,
                    "required": ["action", "ref", "label", "parents", "evidence"],
                    "properties": {
                        "action": { "const": "create_concept" },
                        "ref": {
                            "type": "string",
                            "minLength": 1,
                            "description": "<bazaar:annals.schema.create_concept.ref.description>"
                        },
                        "label": { "type": "string", "minLength": 1 },
                        "parents": {
                            "type": "array",
                            "uniqueItems": true,
                            "items": concept
                        },
                        "evidence": evidence_list
                    }
                },
                {
                    "type": "object",
                    "description": "<bazaar:annals.schema.add_parent.description>",
                    "additionalProperties": false,
                    "required": ["action", "concept", "parent"],
                    "properties": {
                        "action": { "const": "add_parent" },
                        "concept": concept,
                        "parent": concept
                    }
                },
                {
                    "type": "object",
                    "description": "<bazaar:annals.schema.remove_parent.description>",
                    "additionalProperties": false,
                    "required": ["action", "concept", "parent"],
                    "properties": {
                        "action": { "const": "remove_parent" },
                        "concept": concept,
                        "parent": concept
                    }
                },
                {
                    "type": "object",
                    "description": "<bazaar:annals.schema.add_evidence.description>",
                    "additionalProperties": false,
                    "required": ["action", "concept", "evidence"],
                    "properties": {
                        "action": { "const": "add_evidence" },
                        "concept": concept,
                        "evidence": evidence_list
                    }
                },
                {
                    "type": "object",
                    "description": "<bazaar:annals.schema.remove_evidence.description>",
                    "additionalProperties": false,
                    "required": ["action", "concept", "evidence"],
                    "properties": {
                        "action": { "const": "remove_evidence" },
                        "concept": concept,
                        "evidence": evidence_list
                    }
                },
                {
                    "type": "object",
                    "description": "<bazaar:annals.schema.reword_concept.description>",
                    "additionalProperties": false,
                    "required": ["action", "concept", "label", "evidence_disposition"],
                    "properties": {
                        "action": { "const": "reword_concept" },
                        "concept": concept,
                        "label": { "type": "string", "minLength": 1 },
                        "evidence_disposition": {
                            "type": "string",
                            "enum": ["retain", "remove"],
                            "description": "<bazaar:annals.schema.reword_concept.keep_evidence.description>"
                        }
                    }
                },
                {
                    "type": "object",
                    "description": "<bazaar:annals.schema.retire_concept.description>",
                    "additionalProperties": false,
                    "required": ["action", "concept"],
                    "properties": {
                        "action": { "const": "retire_concept" },
                        "concept": concept,
                        "replacement": concept
                    }
                }
            ]
        }
    });
    let annotations = json!({
        "type": "array",
        "description": "<bazaar:annals.schema.annotations.description>",
        "items": { "type": "string", "minLength": 1 }
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "operations"],
        "properties": {
            "summary": { "type": "string", "minLength": 1 },
            "operations": operations,
            "annotations": annotations
        }
    })
}

fn revise_reconciliation_schema() -> Value {
    let submit = submit_reconciliation_schema();
    let operation = submit["properties"]["operations"]["items"].clone();
    let annotations = submit["properties"]["annotations"].clone();
    let operation_id = json!({
        "type": "string",
        "pattern": "^op-[1-9][0-9]*$",
        "description": "<bazaar:annals.schema.operation_id.description>"
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["expected_version"],
        "properties": {
            "expected_version": {
                "type": "integer",
                "minimum": 1,
                "description": "<bazaar:annals.schema.draft_version.description>"
            },
            "replace": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["operation_id", "operation"],
                    "properties": {
                        "operation_id": operation_id,
                        "operation": operation
                    }
                }
            },
            "remove": {
                "type": "array",
                "uniqueItems": true,
                "items": operation_id
            },
            "append": {
                "type": "array",
                "items": operation
            },
            "summary": { "type": "string", "minLength": 1 },
            "annotations": annotations
        },
        "anyOf": [
            { "required": ["replace"] },
            { "required": ["remove"] },
            { "required": ["append"] },
            { "required": ["summary"] },
            { "required": ["annotations"] }
        ]
    })
}

fn reconciliation_status_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "operation_ids": {
                "type": "array",
                "maxItems": 20,
                "uniqueItems": true,
                "items": {
                    "type": "string",
                    "pattern": "^op-[1-9][0-9]*$"
                },
                "description": "<bazaar:annals.schema.reconciliation_status.operation_ids.description>"
            }
        }
    })
}

fn discard_reconciliation_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["expected_version"],
        "properties": {
            "expected_version": { "type": "integer", "minimum": 1 },
            "reason": {
                "type": "string",
                "minLength": 1,
                "description": "<bazaar:annals.schema.discard_reconciliation.reason.description>"
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_inputs_expose_public_concept_ids_without_storage_addresses() {
        fn contains_key(value: &Value, forbidden: &str) -> bool {
            match value {
                Value::Object(object) => {
                    object.contains_key(forbidden)
                        || object.values().any(|value| contains_key(value, forbidden))
                }
                Value::Array(array) => array.iter().any(|value| contains_key(value, forbidden)),
                _ => false,
            }
        }

        let tools = tool_definitions();
        let reconciliation = &tools[5]["inputSchema"];
        assert_eq!(reconciliation["required"], json!(["summary", "operations"]));
        assert!(reconciliation["properties"].get("annotations").is_some());
        assert!(reconciliation["properties"].get("outcome").is_none());
        assert!(reconciliation["properties"].get("uncertainties").is_none());

        let Some(operations) =
            reconciliation["properties"]["operations"]["items"]["oneOf"].as_array()
        else {
            panic!("operations must be alternatives");
        };
        let actions = operations
            .iter()
            .filter_map(|operation| operation.pointer("/properties/action/const")?.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            [
                "create_concept",
                "add_parent",
                "remove_parent",
                "add_evidence",
                "remove_evidence",
                "reword_concept",
                "retire_concept"
            ]
        );
        assert_eq!(
            operations[0]["required"],
            json!(["action", "ref", "label", "parents", "evidence"])
        );

        assert_eq!(
            tools[2]["inputSchema"]["properties"]["queries"]["items"]["type"],
            "string"
        );
        assert_eq!(
            tools[3]["inputSchema"]["properties"]["queries"]["items"]["required"],
            json!(["query"])
        );
        let Some(inspect_requests) =
            tools[4]["inputSchema"]["properties"]["requests"]["items"]["oneOf"].as_array()
        else {
            panic!("inspect requests must be tagged alternatives");
        };
        let inspect_kinds = inspect_requests
            .iter()
            .filter_map(|request| request.pointer("/properties/kind/const")?.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            inspect_kinds,
            [
                "overview", "roots", "concept", "parents", "children", "evidence", "graph"
            ]
        );

        let definitions = Value::Array(tools);
        for forbidden in [
            "work_id",
            "node_id",
            "concept_id",
            "unit_id",
            "base_revision",
            "start_byte",
            "end_byte",
            "position",
            "path",
            "under",
            "before",
            "after",
            "order",
        ] {
            assert!(
                !contains_key(&definitions, forbidden),
                "tool schema exposed forbidden field {forbidden}"
            );
        }
        let definitions = serde_json::to_string(&definitions).unwrap_or_default();
        assert!(definitions.contains("heading_path"));
        assert!(definitions.contains("\"id\""));
        assert!(definitions.contains("\"new\""));
        assert!(!definitions.contains("move_concept"));
        assert!(definitions.contains("\"quote\""));
    }

    #[test]
    fn evidence_selectors_fan_out_but_work_reads_stay_uniquely_anchored() -> cell_prompts::Result<()>
    {
        let mut tools = tool_definitions();
        let prompts = cell_prompts::Prompts::at("annals", 1)?;
        for tool in &mut tools {
            prompts.descriptions(tool)?;
        }
        let work_read = &tools[1];
        assert!(
            work_read["description"]
                .as_str()
                .is_some_and(|description| description.contains("must resolve uniquely"))
        );
        assert!(
            work_read
                .pointer(
                    "/inputSchema/properties/regions/items/properties/around_quote/description",
                )
                .and_then(Value::as_str)
                .is_some_and(|description| description.contains("must resolve uniquely"))
        );

        let evidence = &tools[5]["inputSchema"]["properties"]["operations"]["items"]["oneOf"][0]["properties"]
            ["evidence"]["items"];
        assert!(
            evidence["description"]
                .as_str()
                .is_some_and(|description| description.contains("every occurrence"))
        );
        assert!(
            evidence["description"]
                .as_str()
                .is_some_and(|description| description.contains("bounded fan-out"))
        );
        let instructions = prompts.expand(instructions())?;
        assert!(instructions.contains("selects every occurrence"));
        assert!(instructions.contains("never submit source offsets"));
        assert!(instructions.contains("must resolve uniquely"));
        Ok(())
    }
}
