#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use conversations::{
    AppServerClient, ClientConfig, Error, ListOptions, Role, StderrPolicy, ThreadRef,
};
use tempfile::TempDir;

fn must<T, E: std::fmt::Display>(result: std::result::Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("unexpected error: {error}"),
    }
}

fn fake_codex(directory: &Path) -> std::path::PathBuf {
    let script = directory.join("codex-fake");
    let log = directory.join("requests.jsonl");
    let fixture = r#"#!/bin/sh
set -eu

if [ "${1:-}" = --version ]; then
    printf '%s\n' 'codex-cli 0.9.0-fake'
    exit 0
fi
[ "${1:-}" = app-server ]
[ "${2:-}" = --stdio ]
printf '%s\n' 'fixture app-server diagnostic' >&2

while IFS= read -r line; do
    printf '%s\n' "$line" >>'@LOG@'
    case "$line" in
        *'"method":"initialized"'*) continue ;;
    esac
    id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
    case "$line" in
        *'"method":"initialize"'*)
            printf '{"id":%s,"result":{"userAgent":"codex-fake/1"}}\n' "$id"
            ;;
        *'"method":"thread/list"'*)
            case "$line" in
                *'"archived":true'*)
                    printf '{"id":%s,"result":{"data":[{"id":"root-archived","sessionId":"root-archived","name":"Legacy decisions","preview":"Old choice","cwd":"/archive","source":"vscode","parentThreadId":null,"forkedFromId":null,"cliVersion":"0.8.0","ephemeral":false,"createdAt":10,"updatedAt":100,"status":{"type":"notLoaded"}}],"nextCursor":null}}\n' "$id"
                    ;;
                *'"cursor":"active-next"'*)
                    printf '{"id":%s,"result":{"data":[],"nextCursor":null}}\n' "$id"
                    ;;
                *)
                    printf '{"id":%s,"result":{"data":[{"id":"sub-new","sessionId":"root-active","name":"Subagent work","preview":"child","cwd":"/work","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"root-active","depth":1}}},"parentThreadId":"root-active","forkedFromId":null,"cliVersion":"0.9.0-fake","ephemeral":false,"createdAt":30,"updatedAt":300,"status":{"type":"notLoaded"}},{"id":"old-sub-no-parent","sessionId":"root-active","name":"Old subagent","preview":"old child","cwd":"/work","source":{"subAgent":"review"},"parentThreadId":null,"forkedFromId":null,"cliVersion":"0.9.0-fake","ephemeral":false,"createdAt":29,"updatedAt":290,"status":{"type":"notLoaded"}},{"id":"exec-task","sessionId":"exec-task","name":"Batch work","preview":"exec","cwd":"/work","source":"exec","parentThreadId":null,"forkedFromId":null,"cliVersion":"0.9.0-fake","ephemeral":false,"createdAt":28,"updatedAt":280,"status":{"type":"notLoaded"}},{"id":"root-active","sessionId":"root-active","name":"Current choice","preview":"Choose","cwd":"/work","source":"appServer","parentThreadId":null,"forkedFromId":null,"cliVersion":"0.9.0-fake","ephemeral":false,"createdAt":20,"updatedAt":200,"status":{"type":"active","activeFlags":[]}}],"nextCursor":"active-next"}}\n' "$id"
                    ;;
            esac
            ;;
        *'"method":"thread/turns/list"'*'"threadId":"root-archived"'*)
            printf '{"id":%s,"error":{"code":-32601,"message":"Unknown method"}}\n' "$id"
            ;;
        *'"method":"thread/turns/list"'*'"cursor":"sub-older"'*)
            printf '{"id":%s,"result":{"data":[{"id":"sub-turn-1","itemsView":"full","startedAt":30,"completedAt":31,"status":"completed","items":[{"id":"shared","type":"userMessage","content":[{"type":"text","text":"shared prompt"}]},{"id":"sub-answer","type":"agentMessage","text":"subagent answer"},{"id":"secret-tool","type":"commandExecution","command":"never expose"}]}],"nextCursor":null,"backwardsCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/turns/list"'*'"sortDirection":"desc"'*'"threadId":"sub-new"'*)
            printf '{"id":%s,"result":{"data":[{"id":"sub-turn-2","itemsView":"full","startedAt":31,"completedAt":32,"status":"completed","items":[{"id":"sub-only","type":"agentMessage","text":"finished child"}]}],"nextCursor":"sub-older","backwardsCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/turns/list"'*'"cursor":"sub-more"'*'"threadId":"sub-new"'*)
            printf '{"id":%s,"result":{"data":[{"id":"sub-turn-2","startedAt":31,"completedAt":32,"status":"completed","items":[{"id":"sub-only","type":"agentMessage","text":"finished child"}]}],"nextCursor":null,"backwardsCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/turns/list"'*'"threadId":"sub-new"'*)
            printf '{"id":%s,"result":{"data":[{"id":"sub-turn-1","startedAt":30,"completedAt":31,"status":"completed","items":[{"id":"shared","type":"userMessage","content":[{"type":"text","text":"shared prompt"}]},{"id":"sub-answer","type":"agentMessage","text":"subagent answer"},{"id":"secret-tool","type":"commandExecution","command":"never expose"}]}],"nextCursor":"sub-more","backwardsCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/turns/list"'*'"threadId":"old-sub-no-parent"'*)
            printf '{"id":%s,"result":{"data":[{"id":"old-sub-turn","startedAt":29,"completedAt":30,"status":"completed","items":[{"id":"old-sub-answer","type":"agentMessage","text":"old subagent answer"}]}],"nextCursor":null,"backwardsCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/turns/list"'*'"threadId":"root-active"'*)
            printf '{"id":%s,"result":{"data":[{"id":"active-turn","itemsView":"full","startedAt":20,"completedAt":21,"status":"completed","items":[{"id":"shared","type":"userMessage","content":[{"type":"text","text":"shared prompt"}]},{"id":"active-only","type":"agentMessage","text":"active answer"},{"id":"active-file","type":"fileChange","status":"completed","changes":[{"path":"/private/activity-secret","diff":"ACTIVITY SECRET DIFF","kind":{"type":"update","move_path":null}}]}]}],"nextCursor":null,"backwardsCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/read"'*'"threadId":"root-archived"'*)
            printf '{"id":%s,"result":{"thread":{"id":"root-archived","turns":[{"id":"legacy-turn","startedAt":10,"completedAt":11,"status":"completed","items":[{"id":"legacy-user","type":"userMessage","content":[{"type":"text","text":"legacy decision"}]},{"id":"legacy-answer","type":"agentMessage","text":"accepted"}]}]}}}\n' "$id"
            ;;
        *)
            printf '{"id":%s,"error":{"code":-32601,"message":"unexpected fixture request"}}\n' "$id"
            ;;
    esac
done
"#
    .replace("@LOG@", &log.display().to_string());
    must(fs::write(&script, fixture));
    let mut permissions = must(fs::metadata(&script)).permissions();
    permissions.set_mode(0o755);
    must(fs::set_permissions(&script, permissions));
    script
}

fn fake_codex_with_copied_turn(directory: &Path) -> std::path::PathBuf {
    let script = directory.join("codex-copied-turn-fake");
    let log = directory.join("requests.jsonl");
    let fixture = r#"#!/bin/sh
set -eu

[ "${1:-}" = app-server ]
[ "${2:-}" = --stdio ]
while IFS= read -r line; do
    printf '%s\n' "$line" >>'@LOG@'
    case "$line" in
        *'"method":"initialized"'*) continue ;;
    esac
    id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
    case "$line" in
        *'"method":"initialize"'*)
            printf '{"id":%s,"result":{"userAgent":"codex-copied-fake/1"}}\n' "$id"
            ;;
        *'"method":"thread/list"'*'"archived":true'*)
            printf '{"id":%s,"result":{"data":[],"nextCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/list"'*)
            printf '{"id":%s,"result":{"data":[{"id":"root","sessionId":"session-family","preview":"root","source":"appServer","parentThreadId":null,"forkedFromId":null,"ephemeral":false,"status":{"type":"notLoaded"}},{"id":"fork","sessionId":"session-family","preview":"fork","source":"appServer","parentThreadId":null,"forkedFromId":"root","ephemeral":false,"status":{"type":"notLoaded"}}],"nextCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/turns/list"'*'"threadId":"session-family"'*)
            printf '{"id":%s,"result":{"data":[],"nextCursor":null,"backwardsCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/turns/list"'*'"threadId":"root"'*)
            printf '{"id":%s,"result":{"data":[{"id":"copied","itemsView":"full","startedAt":1,"completedAt":2,"status":"completed","items":[{"id":"root-user","type":"userMessage","content":[{"type":"text","text":"decide"}]}]}],"nextCursor":null,"backwardsCursor":null}}\n' "$id"
            ;;
        *'"method":"thread/turns/list"'*'"threadId":"fork"'*)
            printf '{"id":%s,"result":{"data":[{"id":"copied","itemsView":"full","startedAt":1,"completedAt":2,"status":"completed","items":[{"id":"root-user","type":"userMessage","content":[{"type":"text","text":"decide"}]}]}],"nextCursor":null,"backwardsCursor":null}}\n' "$id"
            ;;
        *)
            printf '{"id":%s,"error":{"code":-32601,"message":"unexpected fixture request"}}\n' "$id"
            ;;
    esac
done
"#
    .replace("@LOG@", &log.display().to_string());
    must(fs::write(&script, fixture));
    let mut permissions = must(fs::metadata(&script)).permissions();
    permissions.set_mode(0o755);
    must(fs::set_permissions(&script, permissions));
    script
}

#[cfg(target_os = "macos")]
fn fake_codex_with_persistent_grandchild(directory: &Path) -> std::path::PathBuf {
    let script = directory.join("codex-wrapper-fake");
    let pid_file = directory.join("app-server.pid");
    let fixture = r#"#!/bin/sh
set -eu

[ "${1:-}" = app-server ]
[ "${2:-}" = --stdio ]
exec 3<&0
(
    trap '' HUP TERM
    while IFS= read -r line <&3; do
        case "$line" in
            *'"method":"initialized"'*) continue ;;
        esac
        id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
        case "$line" in
            *'"method":"initialize"'*)
                printf '{"id":%s,"result":{"userAgent":"codex-grandchild-fake/1"}}\n' "$id"
                ;;
        esac
    done
    while :; do
        sleep 60
    done
) &
printf '%s\n' "$!" >'@PID_FILE@'
exit 0
"#
    .replace("@PID_FILE@", &pid_file.display().to_string());
    must(fs::write(&script, fixture));
    let mut permissions = must(fs::metadata(&script)).permissions();
    permissions.set_mode(0o755);
    must(fs::set_permissions(&script, permissions));
    script
}

#[cfg(target_os = "macos")]
fn process_is_running(pid: u32) -> bool {
    let Ok(output) = Command::new("/bin/ps")
        .args(["-o", "state=", "-p", &pid.to_string()])
        .output()
    else {
        return false;
    };
    let state = String::from_utf8_lossy(&output.stdout);
    let state = state.trim();
    output.status.success() && !state.is_empty() && !state.starts_with('Z')
}

#[cfg(target_os = "macos")]
fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    predicate()
}

fn client(script: std::path::PathBuf) -> AppServerClient {
    must(AppServerClient::spawn(ClientConfig {
        codex_path: script,
        codex_args: Vec::new(),
        host_id: "test-host".to_owned(),
        request_timeout: Duration::from_secs(2),
        stderr_policy: StderrPolicy::Suppress,
    }))
}

#[test]
fn enumerates_both_archives_and_filters_subagents_by_default() {
    let directory = must(TempDir::new());
    let script = fake_codex(directory.path());
    let mut client = client(script);

    let roots = must(client.list(&ListOptions::default()));
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0].reference.thread_id, "root-active");
    assert_eq!(roots[1].reference.thread_id, "root-archived");
    assert!(roots[1].archived);

    let all = must(client.list(&ListOptions {
        include_subagents: true,
        ..ListOptions::default()
    }));
    assert_eq!(all.len(), 4);
    assert_eq!(all[0].source_kind, "subAgentThreadSpawn");
    assert_eq!(all[1].source_kind, "subAgentReview");

    let including_exec = must(client.list(&ListOptions {
        include_subagents: true,
        include_exec: true,
        ..ListOptions::default()
    }));
    assert_eq!(including_exec.len(), 5);

    let recent_limited = must(client.list(&ListOptions {
        include_subagents: true,
        include_exec: true,
        updated_after: Some(250),
        limit: Some(2),
        ..ListOptions::default()
    }));
    assert_eq!(recent_limited.len(), 2);
    assert!(
        recent_limited
            .iter()
            .all(|thread| thread.updated_at.is_some_and(|value| value >= 250))
    );

    let log = must(fs::read_to_string(directory.path().join("requests.jsonl")));
    assert!(
        log.contains("\"sourceKinds\":[\"cli\",\"vscode\",\"exec\",\"appServer\",\"subAgent\"")
    );
    assert!(log.contains("\"archived\":false"));
    assert!(log.contains("\"archived\":true"));
    assert!(log.contains("\"useStateDbOnly\":true"));
}

#[test]
fn exact_thread_summary_uses_canonical_host_and_metadata_only() {
    let directory = must(TempDir::new());
    let script = fake_codex(directory.path());
    let mut client = client(script);

    let summary = must(client.read_thread_summary(&ThreadRef {
        host_id: "test-host".to_owned(),
        thread_id: "root-archived".to_owned(),
    }));
    assert_eq!(summary.reference.host_id, "test-host");
    assert_eq!(summary.reference.thread_id, "root-archived");
    assert_eq!(summary.cwd.as_deref(), Some("/archive"));
    assert!(summary.archived);

    let missing = client.read_thread_summary(&ThreadRef {
        host_id: "test-host".to_owned(),
        thread_id: "missing-thread".to_owned(),
    });
    assert!(matches!(missing, Err(Error::NotFound(thread_id)) if thread_id == "missing-thread"));

    let log = must(fs::read_to_string(directory.path().join("requests.jsonl")));
    assert!(log.contains("\"method\":\"thread/list\""));
    assert!(log.contains("\"archived\":false"));
    assert!(log.contains("\"archived\":true"));
    assert!(log.contains("\"useStateDbOnly\":true"));
    assert!(!log.contains("\"method\":\"thread/turns/list\""));
    assert!(!log.contains("\"method\":\"thread/read\""));
}

#[test]
fn exact_thread_summary_rejects_foreign_host_before_history_read() {
    let directory = must(TempDir::new());
    let script = fake_codex(directory.path());
    let mut client = client(script);

    let result = client.read_thread_summary(&ThreadRef {
        host_id: "another-host".to_owned(),
        thread_id: "root-active".to_owned(),
    });
    assert!(matches!(
        result,
        Err(Error::ThreadHostMismatch {
            reference_host_id,
            client_host_id
        }) if reference_host_id == "another-host" && client_host_id == "test-host"
    ));

    let log = must(fs::read_to_string(directory.path().join("requests.jsonl")));
    assert!(!log.contains("\"method\":\"thread/list\""));
    assert!(!log.contains("\"method\":\"thread/read\""));
}

#[test]
fn paginates_turns_falls_back_for_legacy_and_deduplicates_forks() {
    let directory = must(TempDir::new());
    let script = fake_codex(directory.path());
    let mut client = client(script);

    let snapshot = must(client.snapshot(&ListOptions {
        include_subagents: true,
        ..ListOptions::default()
    }));
    assert_eq!(snapshot.len(), 4);
    let messages = snapshot
        .iter()
        .flat_map(|conversation| &conversation.turns)
        .flat_map(|turn| &turn.messages)
        .collect::<Vec<_>>();
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.reference.item_id == "shared")
            .count(),
        1
    );
    assert!(
        messages
            .iter()
            .any(|message| message.text == "legacy decision")
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.text.contains("never expose"))
    );
    assert_eq!(
        messages
            .iter()
            .find(|message| message.reference.item_id == "legacy-user")
            .map(|message| message.role),
        Some(Role::User)
    );

    let log = must(fs::read_to_string(directory.path().join("requests.jsonl")));
    assert!(log.contains("\"method\":\"thread/turns/list\""));
    assert!(log.contains("\"itemsView\":\"full\""));
    assert!(log.contains("\"cursor\":\"sub-more\""));
    assert!(log.contains("\"method\":\"thread/read\""));
    assert!(log.contains("\"includeTurns\":true"));
}

#[test]
fn exact_and_session_resolved_activity_are_narrow_and_content_free() {
    let exact_directory = must(TempDir::new());
    let exact_script = fake_codex(exact_directory.path());
    let mut exact_client = client(exact_script);

    let exact = must(exact_client.read_turn_activity("root-active", "active-turn"));
    assert_eq!(exact.turn.reference.thread_id, "root-active");
    assert_eq!(exact.turn.status, "completed");
    assert_eq!(exact.turn.completed_at, Some(21));
    assert_eq!(exact.turn.messages.len(), 2);
    assert_eq!(exact.completed_file_changes.len(), 1);
    assert_eq!(exact.completed_file_changes[0].change_count, 1);
    let exact_log = must(fs::read_to_string(
        exact_directory.path().join("requests.jsonl"),
    ));
    assert!(exact_log.contains("\"limit\":100"));
    assert!(exact_log.contains("\"sortDirection\":\"desc\""));
    assert!(exact_log.contains("\"itemsView\":\"full\""));
    assert!(!exact_log.contains("\"method\":\"thread/list\""));

    let resolved_directory = must(TempDir::new());
    let resolved_script = fake_codex(resolved_directory.path());
    let mut resolved_client = client(resolved_script);
    let resolved = must(resolved_client.resolve_turn_activity("root-active", "sub-turn-1"));
    assert_eq!(resolved.turn.reference.thread_id, "sub-new");
    let resolved_log = must(fs::read_to_string(
        resolved_directory.path().join("requests.jsonl"),
    ));
    assert!(resolved_log.contains("\"threadId\":\"sub-new\""));
    assert!(resolved_log.contains("\"threadId\":\"old-sub-no-parent\""));
    assert!(resolved_log.contains("\"useStateDbOnly\":true"));

    let paged_directory = must(TempDir::new());
    let paged_script = fake_codex(paged_directory.path());
    let mut paged_client = client(paged_script);
    let older = must(paged_client.read_turn_activity("sub-new", "sub-turn-1"));
    assert_eq!(older.turn.reference.turn_id, "sub-turn-1");
    let paged_log = must(fs::read_to_string(
        paged_directory.path().join("requests.jsonl"),
    ));
    assert!(paged_log.contains("\"cursor\":\"sub-older\""));

    let legacy_directory = must(TempDir::new());
    let legacy_script = fake_codex(legacy_directory.path());
    let mut legacy_client = client(legacy_script);
    let legacy = must(legacy_client.read_turn_activity("root-archived", "legacy-turn"));
    assert_eq!(legacy.turn.reference.turn_id, "legacy-turn");
    let legacy_log = must(fs::read_to_string(
        legacy_directory.path().join("requests.jsonl"),
    ));
    assert!(legacy_log.contains("\"method\":\"thread/read\""));
}

#[test]
fn completed_activity_reconciliation_uses_full_history_without_changing_corpus() {
    let directory = must(TempDir::new());
    let script = fake_codex(directory.path());
    let mut client = client(script);
    let summary = must(client.list(&ListOptions::default()))
        .into_iter()
        .find(|summary| summary.reference.thread_id == "root-active");
    let Some(summary) = summary else {
        panic!("root-active summary is absent");
    };

    let activities = must(client.read_completed_turn_activities(&summary));
    assert_eq!(activities.len(), 1);
    assert_eq!(activities[0].turn.reference.turn_id, "active-turn");
    assert!(activities[0].has_completed_file_change());

    let conversation = must(client.read(&summary));
    let serialized = must(serde_json::to_string(&conversation));
    assert!(!serialized.contains("completedFileChanges"));
    assert!(!serialized.contains("/private/activity-secret"));
    assert!(!serialized.contains("ACTIVITY SECRET DIFF"));
}

#[test]
fn activity_cli_reports_only_counts_and_stable_references() {
    let directory = must(TempDir::new());
    let script = fake_codex(directory.path());
    let binary = env!("CARGO_BIN_EXE_conversations");
    let output = must(
        Command::new(binary)
            .args([
                "--codex",
                &script.display().to_string(),
                "--app-server-stderr",
                "suppress",
                "activity",
                "root-active",
                "active-turn",
                "--json",
            ])
            .output(),
    );
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"threadId\": \"root-active\""));
    assert!(stdout.contains("\"turnId\": \"active-turn\""));
    assert!(stdout.contains("\"changeCount\": 1"));
    for private in [
        "shared prompt",
        "active answer",
        "/private/activity-secret",
        "ACTIVITY SECRET DIFF",
    ] {
        assert!(!stdout.contains(private));
    }
}

#[test]
fn session_resolution_prefers_a_valid_exact_thread_and_rejects_copied_ambiguity() {
    let exact_directory = must(TempDir::new());
    let exact_script = fake_codex_with_copied_turn(exact_directory.path());
    let mut exact_client = client(exact_script);
    let exact = must(exact_client.resolve_turn_activity("root", "copied"));
    assert_eq!(exact.turn.reference.thread_id, "root");
    let exact_log = must(fs::read_to_string(
        exact_directory.path().join("requests.jsonl"),
    ));
    assert!(!exact_log.contains("\"method\":\"thread/list\""));
    assert!(!exact_log.contains("\"threadId\":\"fork\""));

    let ambiguous_directory = must(TempDir::new());
    let ambiguous_script = fake_codex_with_copied_turn(ambiguous_directory.path());
    let mut ambiguous_client = client(ambiguous_script);
    let result = ambiguous_client.resolve_turn_activity("session-family", "copied");
    assert!(
        matches!(result, Err(Error::AmbiguousTurn { thread_ids, .. }) if thread_ids == "fork, root")
    );
}

#[test]
fn full_text_search_refresh_and_doctor_keep_boundaries_explicit() {
    let directory = must(TempDir::new());
    let script = fake_codex(directory.path());
    let mut client = client(script);

    let hits = must(client.search("legacy", &ListOptions::default()));
    assert!(hits.iter().any(|hit| hit.message.text == "legacy decision"));
    assert!(
        hits.iter()
            .all(|hit| hit.message.reference.item_id != "secret-tool")
    );

    let refresh = must(client.refresh());
    assert_eq!(refresh.total_threads, 5);

    let doctor = must(client.doctor());
    assert!(doctor.ok);
    assert_eq!(
        doctor.app_server_user_agent.as_deref(),
        Some("codex-fake/1")
    );
    assert!(
        doctor
            .warnings
            .iter()
            .any(|warning| warning.contains("0.8.0"))
    );
    assert!(
        doctor
            .warnings
            .iter()
            .any(|warning| warning.contains("notLoaded"))
    );

    let log = must(fs::read_to_string(directory.path().join("requests.jsonl")));
    assert!(log.contains("\"searchTerm\":\"legacy\""));
    assert!(log.contains("\"useStateDbOnly\":false"));
}

#[test]
fn cli_stderr_policy_is_explicit_and_suppressible() {
    assert_eq!(ClientConfig::default().stderr_policy, StderrPolicy::Inherit);

    let directory = must(TempDir::new());
    let script = fake_codex(directory.path());
    let binary = env!("CARGO_BIN_EXE_conversations");
    let inherited = must(
        Command::new(binary)
            .args([
                "--codex",
                &script.display().to_string(),
                "--app-server-stderr",
                "inherit",
                "list",
                "--limit",
                "0",
            ])
            .output(),
    );
    assert!(inherited.status.success());
    assert!(String::from_utf8_lossy(&inherited.stderr).contains("fixture app-server diagnostic"));

    let suppressed = must(
        Command::new(binary)
            .args([
                "--codex",
                &script.display().to_string(),
                "--app-server-stderr",
                "suppress",
                "list",
                "--limit",
                "0",
            ])
            .output(),
    );
    assert!(suppressed.status.success());
    assert!(!String::from_utf8_lossy(&suppressed.stderr).contains("fixture app-server diagnostic"));
}

#[cfg(target_os = "macos")]
#[test]
fn dropping_client_terminates_wrapper_grandchild_process() {
    let directory = must(TempDir::new());
    let script = fake_codex_with_persistent_grandchild(directory.path());
    let client = client(script);
    let pid_file = directory.path().join("app-server.pid");
    assert!(wait_until(Duration::from_secs(1), || pid_file.exists()));
    let pid = must(must(fs::read_to_string(pid_file)).trim().parse::<u32>());
    assert!(process_is_running(pid));

    drop(client);

    assert!(wait_until(Duration::from_secs(2), || !process_is_running(
        pid
    )));
}
