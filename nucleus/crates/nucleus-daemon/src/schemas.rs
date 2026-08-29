pub const JOB_REQUEST_ID: &str = "nucleus.job-request.v1";
pub const BYTES_ID: &str = "nucleus.raw-bytes.v1";
pub const TOOLSET_DEFINITIONS_ID: &str = "nucleus.toolset-definitions.v1";

pub struct InternalSchema {
    pub id: &'static str,
    pub name: &'static str,
    pub document: &'static str,
}

pub const INTERNAL_SCHEMAS: &[InternalSchema] = &[
    InternalSchema {
        id: JOB_REQUEST_ID,
        name: "Nucleus job request v1",
        document: r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"JobRequestV1",
  "type":"object",
  "additionalProperties":false,
  "required":["version","id","label","requester","instructions","prompt","invocation"],
  "properties":{
    "version":{"const":1},
    "id":{"type":"string"},
    "label":{"type":"string"},
    "requester":{"type":"object","additionalProperties":false,"required":["program","id"],"properties":{"program":{"type":"string"},"id":{"type":"string"}}},
    "parent":{"type":"string"},
    "instructions":{"type":"string","minLength":1},
    "prompt":{"type":"string","minLength":1},
    "invocation":{
      "type":"object",
      "additionalProperties":false,
      "required":["version","harness","model","cwd","workspaceAccess","builtinTools","timeoutSeconds"],
      "properties":{
        "version":{"const":1},
        "harness":{"type":"string"},
        "model":{"type":"string"},
        "reasoningEffort":{"enum":["low","medium","high","max"]},
        "cwd":{"type":"string"},
        "workspaceAccess":{"enum":["none","read-only","read-write"]},
        "builtinTools":{
          "type":"object",
          "additionalProperties":false,
          "required":["localExecution","webSearch"],
          "properties":{"localExecution":{"type":"boolean"},"webSearch":{"type":"boolean"}}
        },
        "timeoutSeconds":{"type":"integer","minimum":1},
        "toolset":{
          "type":"object",
          "additionalProperties":false,
          "required":["provider","name","version"],
          "properties":{"provider":{"type":"string"},"name":{"type":"string"},"version":{"type":"integer","minimum":1}}
        }
      }
    }
  }
}"#,
    },
    InternalSchema {
        id: BYTES_ID,
        name: "Nucleus exact byte envelope v1",
        document: r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"RawBytesV1",
  "type":"object",
  "additionalProperties":false,
  "required":["encoding","data"],
  "properties":{
    "encoding":{"const":"base64"},
    "data":{"type":"string","contentEncoding":"base64"}
  }
}"#,
    },
    InternalSchema {
        id: TOOLSET_DEFINITIONS_ID,
        name: "Nucleus toolset definitions v1",
        document: r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"ToolsetDefinitionsV1",
  "type":"object",
  "additionalProperties":false,
  "required":["version","tools"],
  "properties":{
    "version":{"const":1},
    "tools":{"type":"array","minItems":1,"items":{
      "type":"object","additionalProperties":false,
      "required":["name","description","inputSchemaId","inputSchema"],
      "properties":{
        "name":{"type":"string"},
        "description":{"type":"string"},
        "inputSchemaId":{"type":"string"},
        "inputSchema":{"type":"object"}
      }
    }}
  }
}"#,
    },
];

#[cfg(test)]
mod tests {
    use super::INTERNAL_SCHEMAS;

    #[test]
    fn internal_schema_documents_are_valid_json() {
        for schema in INTERNAL_SCHEMAS {
            let document: serde_json::Value = serde_json::from_str(schema.document)
                .unwrap_or_else(|error| panic!("internal schema must be valid JSON: {error}"));
            assert!(document.is_object(), "{} must be a JSON object", schema.id);
        }
    }
}
