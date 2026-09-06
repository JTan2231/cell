use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ModelQuality {
    Low,
    Medium,
    #[default]
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TodoId(i64);

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
pub struct InvalidTodoId;

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
        pub struct $name(i64);

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
        pub struct $error;

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
pub enum TodoStatus {
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
pub struct Todo {
    pub id: TodoId,
    pub title: String,
    pub direction: String,
    pub direction_revision: i64,
    pub status: TodoStatus,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoSummary {
    pub id: TodoId,
    pub title: String,
    pub status: TodoStatus,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
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
pub struct WorkingNote {
    pub id: WorkingNoteId,
    pub todo_id: TodoId,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoConcern {
    pub id: ConcernId,
    pub attached_todo_id: TodoId,
    pub body: String,
    pub source_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_item_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SituationAssessmentSummary {
    pub id: SituationAssessmentId,
    pub disposition: String,
    pub subject_label: String,
    pub summary: String,
    pub observed_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSummary {
    pub id: DesignId,
    pub revision: i64,
    pub state: String,
    pub summary: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoView {
    pub requested_id: TodoId,
    pub resolution_path: Vec<TodoId>,
    #[serde(flatten)]
    pub todo: Todo,
    pub concerns: Vec<TodoConcern>,
    pub working_notes: Vec<WorkingNote>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_assessment: Option<SituationAssessmentSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_design: Option<DesignSummary>,
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
