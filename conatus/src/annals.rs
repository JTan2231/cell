use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use ::annals::api::{
    CliClient, CliLibraryKind, ConceptDetail, ConceptId, GraphEdge, InboxCommand, InboxEnqueueArgs,
    InboxRetryCommand, InboxRetryStartArgs, InboxRetryWindowArgs, InboxRunArgs,
    InstructionsCommand, InstructionsSetArgs, IntegrateArgs, LibraryCommand, LibraryCreateArgs,
    LibraryReader, LogArgs, Request, Response, WorkCommand, WorkShowArgs,
};
use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::{Value, json};

const CONCEPT_LIMIT: usize = 200;
const PREVIEW_LIMIT: usize = 100;

pub struct Annals {
    client: CliClient,
    library_name: String,
}

impl Annals {
    #[must_use]
    pub fn new(
        executable: PathBuf,
        state_root: Option<PathBuf>,
        library_name: String,
        library_id: Option<String>,
    ) -> Self {
        let mut client = CliClient::new(executable).for_named_library(&library_name);
        client.state_root = state_root;
        client.expected_library_id = library_id;
        Self {
            client,
            library_name,
        }
    }

    pub fn create(&self) -> Result<String> {
        let mut catalog = self.client.clone();
        catalog.named_library = None;
        catalog.expected_library_id = None;
        let response = catalog.call(&Request::Library(LibraryCommand::Create(
            LibraryCreateArgs {
                name: self.library_name.clone(),
                kind: CliLibraryKind::General,
            },
        )))?;
        let Response::LibraryCreated(library) = response else {
            bail!("Annals did not return a created library");
        };
        if let Some(expected) = &self.client.expected_library_id {
            anyhow::ensure!(
                expected == &library.library_id,
                "the named Annals library does not match the configured library identity"
            );
        }
        Ok(library.library_id)
    }

    pub fn instructions(&self, content: &str) -> Result<Value> {
        let request = Request::Instructions(InstructionsCommand::Set(InstructionsSetArgs {
            content: None,
            file: None,
            stdin: true,
        }));
        match self
            .client
            .call_with_input(&request, Some(content.as_bytes()))?
        {
            Response::InstructionsSet(result) => Ok(serde_json::to_value(result)?),
            _ => bail!("Annals did not return an instruction selection"),
        }
    }

    pub fn current_instructions(&self) -> Result<Value> {
        match self
            .client
            .call(&Request::Instructions(InstructionsCommand::Show))?
        {
            Response::Instructions(result) => Ok(serde_json::to_value(result)?),
            _ => bail!("Annals did not return the selected instructions"),
        }
    }

    pub fn examination(&self, work_name: &str) -> Result<Value> {
        let reader = self.reader()?;
        let reconciliations: Vec<_> = reader
            .reconciliations()?
            .into_iter()
            .filter(|record| record.work == work_name)
            .collect();
        Ok(json!({
            "work": work_name,
            "recorded_reconciliation_count": reconciliations.len(),
            "reconciliations": reconciliations,
            "interpretation": "These are durable reconciliation records for the selected work. An empty collection does not rule out an unfinished or failed examination attempt.",
        }))
    }

    pub fn enqueue(&self, path: &Path) -> Result<Value> {
        match self
            .client
            .call(&Request::Inbox(InboxCommand::Enqueue(InboxEnqueueArgs {
                inputs: vec![path.to_owned()],
                priority: false,
            })))? {
            Response::Enqueued(result) => Ok(serde_json::to_value(result)?),
            _ => bail!("Annals did not return an enqueue receipt"),
        }
    }

    pub fn run(&self) -> Result<Value> {
        match self
            .client
            .call(&Request::Inbox(InboxCommand::Run(InboxRunArgs {
                settle_seconds: None,
            })))? {
            Response::InboxRun(result) => Ok(serde_json::to_value(result)?),
            _ => bail!("Annals did not return an inbox run result"),
        }
    }

    pub fn status(&self) -> Result<Value> {
        match self.client.call(&Request::Inbox(InboxCommand::Status))? {
            Response::InboxStatus(result) => Ok(serde_json::to_value(result)?),
            _ => bail!("Annals did not return inbox status"),
        }
    }

    pub fn work(&self, name: &str) -> Result<Value> {
        match self
            .client
            .call(&Request::Work(WorkCommand::Show(WorkShowArgs {
                label: name.to_owned(),
            })))? {
            Response::Work(result) => Ok(serde_json::to_value(result)?),
            _ => bail!("Annals did not return a retained work"),
        }
    }

    pub fn graph(&self) -> Result<Value> {
        Ok(serde_json::to_value(self.snapshot()?)?)
    }

    pub fn history(&self, limit: usize) -> Result<Value> {
        match self.client.call(&Request::Log(LogArgs { limit }))? {
            Response::Log(result) => Ok(serde_json::to_value(result)?),
            _ => bail!("Annals did not return corpus history"),
        }
    }

    pub fn retry(&self, from: &str, through: &str) -> Result<Value> {
        match self.client.call(&Request::Inbox(InboxCommand::Retry(
            InboxRetryCommand::Start(InboxRetryStartArgs {
                window: InboxRetryWindowArgs {
                    from_job_id: from.to_owned(),
                    through_job_id: through.to_owned(),
                },
                reason: Some("Conatus requested retry of the selected failed jobs.".to_owned()),
            }),
        )))? {
            Response::RetryEvent(result) => Ok(serde_json::to_value(result)?),
            _ => bail!("Annals did not return a retry event"),
        }
    }

    pub fn reexamine(&self, work: &str) -> Result<Value> {
        match self.client.call(&Request::Integrate(IntegrateArgs {
            input: None,
            work: Some(work.to_owned()),
            name: None,
            quality: None,
            model: None,
            reexamine: true,
            apply: true,
        }))? {
            Response::Reconciliation(result) => Ok(serde_json::to_value(result)?),
            Response::Applied(result) => Ok(serde_json::to_value(result)?),
            _ => bail!("Annals did not return a reconciliation result"),
        }
    }

    pub fn associations(&self, work_name: &str) -> Result<Value> {
        let snapshot = self.snapshot()?;
        let grounded: Vec<_> = snapshot
            .concepts
            .iter()
            .filter(|concept| {
                concept
                    .evidence
                    .items
                    .iter()
                    .any(|evidence| evidence.work == work_name)
            })
            .map(|concept| {
                json!({
                    "concept_id": concept.summary.id,
                    "source_evidence": concept.evidence.items.iter()
                        .filter(|evidence| evidence.work == work_name).collect::<Vec<_>>(),
                    "serves": related(concept.summary.id, &snapshot, true),
                    "served_by": related(concept.summary.id, &snapshot, false),
                })
            })
            .collect();
        Ok(json!({
            "work": work_name,
            "grounded_concepts": grounded,
            "graph": snapshot,
            "interpretation": "A child is interpreted as serving its parent. Evidence grounds concepts, not individual edges. Paths report reachability in the returned graph.",
        }))
    }

    fn reader(&self) -> Result<LibraryReader> {
        // Named selection verifies the configured identity before exposing the
        // library's supported read handle. No private database rows are read here.
        match self.client.call(&Request::Show)? {
            Response::Library(selected) => Ok(LibraryReader::open(&selected.library.library)?),
            _ => bail!("Annals did not return the selected library"),
        }
    }

    fn snapshot(&self) -> Result<Snapshot> {
        let reader = self.reader()?;
        let overview = reader.overview(None)?;
        let revision = overview.revision;
        let roots = reader.roots(Some(revision), CONCEPT_LIMIT, None)?;
        let mut queue: VecDeque<_> = roots.roots.items.iter().map(|root| root.id).collect();
        let mut selected = BTreeMap::new();
        let mut relationships_complete = true;
        let mut evidence_complete = true;
        while selected.len() < CONCEPT_LIMIT {
            let Some(id) = queue.pop_front() else {
                break;
            };
            if selected.contains_key(&id) {
                continue;
            }
            let concept = reader.concept(Some(revision), id, PREVIEW_LIMIT)?.concept;
            relationships_complete &= concept.parents.page.next_cursor.is_none()
                && concept.children.page.next_cursor.is_none();
            evidence_complete &= concept.evidence.page.next_cursor.is_none();
            queue.extend(concept.children.items.iter().map(|child| child.id));
            selected.insert(id, concept);
        }

        let mut edge_ids = BTreeSet::new();
        for (id, concept) in &selected {
            for parent in &concept.parents.items {
                if selected.contains_key(&parent.id) {
                    edge_ids.insert((parent.id, *id));
                }
            }
            for child in &concept.children.items {
                if selected.contains_key(&child.id) {
                    edge_ids.insert((*id, child.id));
                }
            }
        }
        let concepts_complete = selected.len() as u64 == overview.concept_count;
        Ok(Snapshot {
            revision,
            total_concepts_at_revision: overview.concept_count,
            returned_concepts: selected.len(),
            concept_limit: CONCEPT_LIMIT,
            preview_limit_per_concept: PREVIEW_LIMIT,
            concepts_complete,
            relationships_complete: concepts_complete && relationships_complete,
            evidence_complete: concepts_complete && evidence_complete,
            concepts: selected.into_values().collect(),
            edges: edge_ids
                .into_iter()
                .map(|(parent_id, child_id)| GraphEdge {
                    parent_id,
                    child_id,
                })
                .collect(),
        })
    }
}

#[derive(Serialize)]
struct Snapshot {
    revision: i64,
    total_concepts_at_revision: u64,
    returned_concepts: usize,
    concept_limit: usize,
    preview_limit_per_concept: usize,
    concepts_complete: bool,
    relationships_complete: bool,
    evidence_complete: bool,
    concepts: Vec<ConceptDetail>,
    edges: Vec<GraphEdge>,
}

fn related(seed: ConceptId, graph: &Snapshot, parents: bool) -> Vec<Value> {
    let mut distances = BTreeMap::from([(seed, 0usize)]);
    let mut queue = VecDeque::from([seed]);
    while let Some(id) = queue.pop_front() {
        let distance = distances[&id] + 1;
        for edge in &graph.edges {
            let (from, to) = if parents {
                (edge.child_id, edge.parent_id)
            } else {
                (edge.parent_id, edge.child_id)
            };
            if from == id && !distances.contains_key(&to) {
                distances.insert(to, distance);
                queue.push_back(to);
            }
        }
    }
    distances
        .into_iter()
        .filter(|(id, _)| *id != seed)
        .filter_map(|(id, distance)| {
            let concept = graph
                .concepts
                .iter()
                .find(|concept| concept.summary.id == id)?;
            let source_works: BTreeSet<_> = concept
                .evidence
                .items
                .iter()
                .map(|evidence| &evidence.work)
                .collect();
            Some(json!({
                "concept_id": id,
                "label": concept.summary.label,
                "source_works": source_works,
                "evidence": concept.evidence.items,
                "evidence_complete": concept.evidence.page.next_cursor.is_none(),
                "relationship": if distance == 1 { "direct" } else { "path" },
                "shortest_returned_path_hops": distance,
            }))
        })
        .collect()
}
