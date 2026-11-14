use super::*;
use crate::r::net::pwg_raster::tests::document;

fn upload(queue: &mut PrintQueue, owner: u8, bytes: &[u8]) -> u32 {
    let id = queue.begin(owner, bytes.len(), 0).unwrap();
    for (index, chunk) in bytes.chunks(137).enumerate() {
        assert_eq!(queue.append(owner, id, index * 137, chunk, 1), Ok(id));
    }
    id
}

#[test]
fn partial_uploads_never_enter_pending_queue_and_commit_is_single_use() {
    let mut queue = PrintQueue::new();
    let bytes = document(2);
    let id = queue.begin(7, bytes.len(), 0).unwrap();
    assert_eq!(queue.records[0].state, PrintJobState::Rendering);
    assert!(queue.pending.is_empty());
    assert_eq!(queue.commit(7, id, 1), Err(ERROR_INVALID_DOCUMENT));
    assert_eq!(queue.append(7, id, 0, &bytes, 1), Ok(id));
    assert_eq!(queue.commit(7, id, 1), Ok(id));
    assert_eq!(queue.records[0].state, PrintJobState::Queued);
    assert_eq!(queue.pending.len(), 1);
    assert_eq!(queue.commit(7, id, 1), Err(ERROR_INVALID_DOCUMENT));
    assert_eq!(queue.pending.len(), 1);
    let job = queue.pending.pop_front().unwrap();
    assert_eq!(job.id, id);
    assert_eq!(job.owner, PrintOwner::Blueprint(7));
    assert_eq!(job.document, bytes);
}

#[test]
fn foreign_owners_cannot_append_commit_abort_or_query() {
    let mut queue = PrintQueue::new();
    let bytes = document(1);
    let id = upload(&mut queue, 7, &bytes);
    assert_eq!(queue.owned_record(8, id), Err(ERROR_NOT_OWNER));
    assert_eq!(
        queue.append(8, id, bytes.len(), &[0], 1),
        Err(ERROR_NOT_OWNER)
    );
    assert_eq!(queue.commit(8, id, 1), Err(ERROR_NOT_OWNER));
    assert_eq!(queue.abort(8, id), Err(ERROR_NOT_OWNER));
    assert_eq!(queue.commit(7, id, 1), Ok(id));
}

#[test]
fn out_of_order_empty_and_oversized_chunks_are_rejected_without_mutation() {
    let mut queue = PrintQueue::new();
    let bytes = document(1);
    let id = queue.begin(1, bytes.len(), 0).unwrap();
    assert_eq!(
        queue.append(1, id, 1, &bytes[..1], 1),
        Err(ERROR_INVALID_DOCUMENT)
    );
    assert_eq!(queue.append(1, id, 0, &[], 1), Err(ERROR_INVALID_DOCUMENT));
    assert_eq!(
        queue.append(1, id, 0, &[0; 10_000], 1),
        Err(ERROR_INVALID_DOCUMENT)
    );
    assert!(queue.uploads[0].bytes.is_empty());
    assert_eq!(queue.append(1, id, 0, &bytes[..100], 1), Ok(id));
    assert_eq!(
        queue.append(1, id, 0, &bytes[..100], 1),
        Err(ERROR_INVALID_DOCUMENT)
    );
    assert_eq!(queue.uploads[0].bytes.len(), 100);
}

#[test]
fn invalid_complete_documents_fail_without_any_pending_work() {
    let mut queue = PrintQueue::new();
    let mut bytes = document(1);
    bytes[0] = 0;
    let id = upload(&mut queue, 1, &bytes);
    assert_eq!(queue.commit(1, id, 1), Err(ERROR_INVALID_DOCUMENT));
    assert_eq!(queue.records[0].state, PrintJobState::Failed);
    assert!(queue.pending.is_empty());
    assert!(queue.uploads.is_empty());
}

#[test]
fn active_queue_is_bounded_and_aborting_or_expiry_releases_upload_memory() {
    let mut queue = PrintQueue::new();
    let bytes = document(1);
    let ids: Vec<_> = (0..QUEUE_CAPACITY)
        .map(|_| upload(&mut queue, 1, &bytes))
        .collect();
    assert_eq!(queue.begin(1, bytes.len(), 1), Err(ERROR_QUEUE_FULL));
    assert_eq!(queue.abort(1, ids[0]), Ok(ids[0]));
    assert_eq!(queue.abort(1, ids[0]), Ok(ids[0]));
    let fresh = queue.begin(1, bytes.len(), 2).unwrap();
    queue.expire_uploads(UPLOAD_IDLE_TIMEOUT_MS);
    assert_eq!(queue.uploads.len(), QUEUE_CAPACITY);
    queue.expire_uploads(UPLOAD_IDLE_TIMEOUT_MS + 2);
    assert!(queue.uploads.is_empty());
    assert_eq!(
        queue
            .records
            .iter()
            .find(|record| record.id == fresh)
            .unwrap()
            .state,
        PrintJobState::Canceled
    );
    assert!(queue.pending.is_empty());
}

#[test]
fn status_retention_keeps_active_jobs_and_identifiers_never_use_zero() {
    let mut queue = PrintQueue::new();
    let bytes = document(1);
    let active = queue.begin(1, bytes.len(), 0).unwrap();
    for _ in 0..STATUS_CAPACITY + 5 {
        let id = queue.begin(1, bytes.len(), 0).unwrap();
        queue.abort(1, id).unwrap();
    }
    assert!(queue.owned_record(1, active).is_ok());
    assert_eq!(queue.records.len(), STATUS_CAPACITY);
    queue.next_id = u32::MAX;
    let last = queue.begin(1, bytes.len(), 0).unwrap();
    assert_eq!(last, u32::MAX);
    let wrapped = queue.begin(1, bytes.len(), 0).unwrap();
    assert_ne!(wrapped, 0);
    assert_ne!(wrapped, active);
}

#[test]
fn native_submission_wire_admits_exact_document_and_reports_owned_outcome() {
    *PRINT_QUEUE.lock() = PrintQueue::new();
    let bytes = document(1);
    let mut header = 1u32.to_le_bytes().to_vec();
    header.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    assert_eq!(
        submit_for_owner(7, PWG_BEGIN, 1, &header),
        ERROR_INVALID_DOCUMENT
    );
    assert_eq!(
        submit_for_owner(7, PWG_BEGIN, 0, &header[..7]),
        ERROR_INVALID_DOCUMENT
    );
    assert_eq!(submit_for_owner(7, 1, 0, &bytes), ERROR_INVALID_DOCUMENT);
    let id = submit_for_owner(7, PWG_BEGIN, 0, &header) as u32;
    assert!(id > 0);
    assert_eq!(status_for_owner(7, id), PrintJobState::Rendering as i32);
    assert_eq!(status_for_owner(8, id), ERROR_NOT_OWNER as i32);
    assert!(take_next_job().is_none());
    let mut chunk = 0u32.to_le_bytes().to_vec();
    chunk.extend_from_slice(&bytes);
    assert_eq!(
        submit_for_owner(7, PWG_APPEND, u64::from(u32::MAX) + 1, &chunk),
        ERROR_INVALID_DOCUMENT
    );
    assert_eq!(
        submit_for_owner(7, PWG_APPEND, u64::from(id), &chunk),
        i64::from(id)
    );
    assert_eq!(
        submit_for_owner(8, PWG_COMMIT, u64::from(id), &[]),
        ERROR_NOT_OWNER
    );
    assert_eq!(
        submit_for_owner(7, PWG_COMMIT, u64::from(id), &[]),
        i64::from(id)
    );
    let job = take_next_job().unwrap();
    assert_eq!(job.document, bytes);
    assert!(take_next_job().is_none());
    transition(id, PrintJobState::Submitted, "test-accepted");
    assert_eq!(status_for_owner(7, id), PrintJobState::Submitted as i32);
    transition(id, PrintJobState::OutcomeUnknown, "test-uncertain");
    transition(id, PrintJobState::Completed, "test-late-response");
    assert_eq!(
        status_for_owner(7, id),
        PrintJobState::OutcomeUnknown as i32
    );
    assert_eq!(
        submit_for_owner(7, PWG_COMMIT, u64::from(id), &[]),
        ERROR_INVALID_DOCUMENT
    );
    assert!(take_next_job().is_none());
}
