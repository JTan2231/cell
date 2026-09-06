use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::ListJobsQueryV1;

// A dead synchronous CLI releases its file lock while an admitted Nucleus job
// may still exist. Count every Todo requester job, including other databases,
// rather than claiming that a process lock alone proves runtime settlement.
pub(crate) fn unfinished_runtime_jobs() -> Option<usize> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let client = NucleusClient::for_current_user()?;
            let mut count = 0;
            // Page by stable job identity, then classify each observed state.
            // Separate state queries can miss a waiting-to-running transition.
            let mut query = ListJobsQueryV1 {
                requester_program: Some("todo".into()),
                limit: Some(1000),
                ..Default::default()
            };
            loop {
                let page = client.list_jobs(&query).await?;
                count += page
                    .jobs
                    .iter()
                    .filter(|job| !job.state.is_terminal())
                    .count();
                if page.next.is_none() {
                    break;
                }
                query.after = page.next;
            }
            Ok::<_, ClientError>(count)
        })
        .await
        .ok()?
        .ok()
    })
}
