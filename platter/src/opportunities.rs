//! Local read projection for prepared opportunities; never retrieves a posting.
use crate::{
    api::{Opportunity, OpportunityList, PacketSummary},
    store::Store,
};
use anyhow::Result;
use rusqlite::params;
use std::path::Path;

pub fn list(root: &Path, query: Option<&str>) -> Result<OpportunityList> {
    let store = Store::open_read_only(root)?;
    let tx = store.connection.unchecked_transaction()?;
    let mut items = Vec::new();
    for job in store.jobs()? {
        let mut statement = store.connection.prepare(
            "SELECT r.id,r.created_at,r.status,
             EXISTS(SELECT 1 FROM artifacts a WHERE a.run_id=r.id AND a.kind='resume-pdf'),
             json_extract(r.inputs,'$.job.url'),json_extract(r.inputs,'$.posting.url')
             FROM runs r WHERE r.opportunity=?1 ORDER BY r.created_at,r.id",
        )?;
        let mut rows = statement.query(params![job.opportunity])?;
        let mut packets = Vec::new();
        let mut urls = Vec::new();
        while let Some(row) = rows.next()? {
            packets.push(PacketSummary {
                id: row.get(0)?,
                created_at: row.get(1)?,
                preparation_status: row.get(2)?,
                has_resume: row.get(3)?,
            });
            for column in [4, 5] {
                if let Some(url) = row.get::<_, Option<String>>(column)?
                    && !urls.contains(&url)
                {
                    urls.push(url);
                }
            }
        }
        // A retrieval failure can leave a job with no preparation or captured URL.
        if packets.is_empty() {
            continue;
        }
        let item = Opportunity {
            reference: job.opportunity,
            cast_job_id: job.cast_job_id,
            company: job.company,
            title: job.title,
            urls,
            packets,
        };
        if query.is_none_or(|q| matches(&item, q)) {
            items.push(item);
        }
    }
    tx.commit()?;
    Ok(OpportunityList {
        schema_version: 1,
        items,
    })
}

#[must_use]
pub fn matches(item: &Opportunity, query: &str) -> bool {
    if let Ok(url) = url::Url::parse(query)
        && matches!(url.scheme(), "https" | "http")
    {
        return item
            .urls
            .iter()
            .any(|stored| crate::source::retained_url_matches(stored, query));
    }
    let query = query.to_lowercase();
    [
        &item.reference,
        &item.cast_job_id,
        &item.company,
        &item.title,
    ]
    .into_iter()
    .chain(item.urls.iter())
    .any(|value| value.to_lowercase().contains(&query))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::PacketRecord;

    #[test]
    fn lookup_groups_regenerations_and_reads_closed_retained_urls() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open(temp.path())?;
        for id in ["first", "regenerated"] {
            store.insert_run(
                &PacketRecord {
                    id: id.into(),
                    opportunity: "ashby:acme:role".into(),
                    job_id: "cast-id".into(),
                    company: "Acme".into(),
                    title: "Engineer".into(),
                    status: "stale".into(),
                    directory: String::new(),
                },
                &serde_json::json!({"job":{"url":"https://jobs.ashbyhq.com/acme/role"}}),
            )?;
        }
        store.set_eligible("cast-id", false)?;
        let before = std::fs::read(temp.path().join(crate::store::DATABASE))?;
        let result = list(
            temp.path(),
            Some("https://jobs.ashbyhq.com/acme/role/application?source=test"),
        )?;
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].packets.len(), 2);
        assert_eq!(result.items[0].reference, "ashby:acme:role");
        assert!(
            list(temp.path(), Some("https://jobs.ashbyhq.com/acme/role-two"))?
                .items
                .is_empty()
        );
        assert_eq!(
            before,
            std::fs::read(temp.path().join(crate::store::DATABASE))?
        );
        Ok(())
    }
}
