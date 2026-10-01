#[test]
fn current_view_retains_retirement_replacement_and_complete_distinctions() {
    use semantics::api::{Concept, Distinction, Repository, RepositoryView};
    let meaning = "Meaning with significant qualifiers. ".repeat(100);
    let statement = "A distinct concept, not a synonym. ".repeat(100);
    let concept = Concept {
        id: "retired".into(),
        label: "Retired concept".into(),
        meaning: meaning.clone(),
        active: false,
        replacement_concept_id: Some("replacement".into()),
        created_revision: 1,
        changed_revision: 4,
        grounds: vec![],
        distinctions: vec![Distinction {
            revision: 3,
            other_concept_id: "other".into(),
            statement: statement.clone(),
        }],
    };
    let repository = Repository {
        project_id: "fixture".into(),
        revision: 4,
        concepts: [(concept.id.clone(), concept)].into(),
    };
    let view = RepositoryView::from(&repository);
    let summary = &view.concepts["retired"];
    assert_eq!(view.revision, 4);
    assert!(!summary.active);
    assert_eq!(
        summary.replacement_concept_id.as_deref(),
        Some("replacement")
    );
    assert_eq!(summary.meaning, meaning);
    assert_eq!(summary.distinctions[0].statement, statement);
}
