use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ModelQuality {
    Low,
    Medium,
    #[default]
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TodoId(i64);

impl TodoId {
    pub(crate) fn from_storage(value: i64) -> Result<Self, InvalidTodoId> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(InvalidTodoId)
        }
    }

    #[must_use]
    pub(crate) const fn storage_id(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidTodoId;

impl fmt::Display for InvalidTodoId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a todo ID must be t followed by a positive decimal integer")
    }
}

impl std::error::Error for InvalidTodoId {}

impl fmt::Display for TodoId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "t{}", self.0)
    }
}

impl FromStr for TodoId {
    type Err = InvalidTodoId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let digits = value.strip_prefix('t').ok_or(InvalidTodoId)?;
        if digits.is_empty()
            || digits.starts_with('0')
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(InvalidTodoId);
        }
        Self::from_storage(digits.parse::<i64>().map_err(|_| InvalidTodoId)?)
    }
}

impl Serialize for TodoId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TodoId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TodoIdVisitor;

        impl Visitor<'_> for TodoIdVisitor {
            type Value = TodoId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a todo ID such as \"t42\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(TodoIdVisitor)
    }
}

macro_rules! prefixed_id {
    ($name:ident, $error:ident, $prefix:literal, $example:literal, $description:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) struct $name(i64);

        impl $name {
            pub(crate) fn from_storage(value: i64) -> Result<Self, $error> {
                if value > 0 {
                    Ok(Self(value))
                } else {
                    Err($error)
                }
            }

            #[must_use]
            pub(crate) const fn storage_id(self) -> i64 {
                self.0
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) struct $error;

        impl fmt::Display for $error {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str($description)
            }
        }

        impl std::error::Error for $error {}

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, concat!($prefix, "{}"), self.0)
            }
        }

        impl FromStr for $name {
            type Err = $error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let digits = value.strip_prefix($prefix).ok_or($error)?;
                if digits.is_empty()
                    || digits.starts_with('0')
                    || !digits.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err($error);
                }
                Self::from_storage(digits.parse::<i64>().map_err(|_| $error)?)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct IdVisitor;

                impl Visitor<'_> for IdVisitor {
                    type Value = $name;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str(concat!("an ID such as ", $example))
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                    where
                        E: de::Error,
                    {
                        value.parse().map_err(E::custom)
                    }
                }

                deserializer.deserialize_str(IdVisitor)
            }
        }
    };
}

prefixed_id!(
    ConcernId,
    InvalidConcernId,
    "c",
    "\"c42\"",
    "a concern ID must be c followed by a positive decimal integer"
);
prefixed_id!(
    RoutingProposalId,
    InvalidRoutingProposalId,
    "r",
    "\"r42\"",
    "a routing proposal ID must be r followed by a positive decimal integer"
);
prefixed_id!(
    SituationAssessmentId,
    InvalidSituationAssessmentId,
    "a",
    "\"a42\"",
    "a situation assessment ID must be a followed by a positive decimal integer"
);
prefixed_id!(
    DesignId,
    InvalidDesignId,
    "d",
    "\"d42\"",
    "a design ID must be d followed by a positive decimal integer"
);
prefixed_id!(
    WorkingNoteId,
    InvalidWorkingNoteId,
    "n",
    "\"n42\"",
    "a working-note ID must be n followed by a positive decimal integer"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TodoStatus {
    Open,
    Done,
}

impl fmt::Display for TodoStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Open => "open",
            Self::Done => "done",
        })
    }
}

impl FromStr for TodoStatus {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "open" => Ok(Self::Open),
            "done" => Ok(Self::Done),
            _ => Err("invalid stored todo status"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Todo {
    pub(crate) id: TodoId,
    pub(crate) title: String,
    pub(crate) direction: String,
    pub(crate) direction_revision: i64,
    pub(crate) status: TodoStatus,
    pub(crate) created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TodoSummary {
    pub(crate) id: TodoId,
    pub(crate) title: String,
    pub(crate) status: TodoStatus,
    pub(crate) created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) completed_at: Option<String>,
}

impl From<&Todo> for TodoSummary {
    fn from(todo: &Todo) -> Self {
        Self {
            id: todo.id,
            title: todo.title.clone(),
            status: todo.status,
            created_at: todo.created_at.clone(),
            completed_at: todo.completed_at.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkingNote {
    pub(crate) id: WorkingNoteId,
    pub(crate) todo_id: TodoId,
    pub(crate) text: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TodoConcern {
    pub(crate) id: ConcernId,
    pub(crate) attached_todo_id: TodoId,
    pub(crate) body: String,
    pub(crate) source_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_item_id: Option<String>,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SituationAssessmentSummary {
    pub(crate) id: SituationAssessmentId,
    pub(crate) disposition: String,
    pub(crate) subject_label: String,
    pub(crate) summary: String,
    pub(crate) observed_at: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignSummary {
    pub(crate) id: DesignId,
    pub(crate) revision: i64,
    pub(crate) state: String,
    pub(crate) summary: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TodoView {
    pub(crate) requested_id: TodoId,
    pub(crate) resolution_path: Vec<TodoId>,
    #[serde(flatten)]
    pub(crate) todo: Todo,
    pub(crate) concerns: Vec<TodoConcern>,
    pub(crate) working_notes: Vec<WorkingNote>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) latest_assessment: Option<SituationAssessmentSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) latest_design: Option<DesignSummary>,
}

#[cfg(test)]
mod tests {
    use super::{
        ConcernId, DesignId, RoutingProposalId, SituationAssessmentId, TodoId, WorkingNoteId,
    };

    #[test]
    fn todo_ids_are_strict_prefixed_identifiers() {
        for value in [1, 42, i64::MAX] {
            let id = TodoId::from_storage(value).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(id.to_string(), format!("t{value}"));
            assert_eq!(id.to_string().parse::<TodoId>(), Ok(id));
        }
        for value in ["", "t", "t0", "t01", "T1", "1", "t-1", " t1"] {
            assert!(value.parse::<TodoId>().is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn domain_ids_keep_distinct_public_prefixes() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!("c7".parse::<ConcernId>()?.to_string(), "c7");
        assert_eq!("r7".parse::<RoutingProposalId>()?.to_string(), "r7");
        assert_eq!("a7".parse::<SituationAssessmentId>()?.to_string(), "a7");
        assert_eq!("d7".parse::<DesignId>()?.to_string(), "d7");
        assert_eq!("n7".parse::<WorkingNoteId>()?.to_string(), "n7");
        assert!("t7".parse::<ConcernId>().is_err());
        assert!("c07".parse::<ConcernId>().is_err());
        Ok(())
    }
}
