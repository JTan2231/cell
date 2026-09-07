# Cast vocabulary

- **Company:** a stored company record keyed by a discovered domain or an ATS
  provider/tenant identity. New means first stored by Cast.
- **Job:** a stored posting with a source identity, extracted fields,
  observation timestamps and recorded availability status.
- **Source:** a careers collection endpoint associated with a company record.
- **Observation:** source-attributed information retained at a known time.
- **Coverage:** the pages or items processed by a query or source collection,
  including its limits and partial, failed or deferred steps.
- **Freshness:** elapsed time since a successful collection, stored separately
  from the latest attempt.
- **Local budget:** Cast's conservative counter for request admission. Each
  provider manages its own billing and available balance.
- **Snapshot:** one consistent view of stored companies, jobs, source metadata,
  revisions and collection records.
