use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_stream::stream;
use axum::{
    extract::{Path, Query, State},
    http::{Method, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, patch, post, put},
    Json, Router,
};
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::{
    sync::{broadcast, mpsc, RwLock},
    time::{sleep, Duration},
};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

const SCHEMA: &str = "filetree.v1";

type JobId = Uuid;
type NodeId = String;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tree = Arc::new(RwLock::new(TreeSnapshot::demo()));
    let jobs = Arc::new(RwLock::new(HashMap::new()));
    let (queue, receiver) = mpsc::channel(128);
    let (events, _) = broadcast::channel(512);

    let app_state = AppState {
        tree: Arc::clone(&tree),
        jobs: Arc::clone(&jobs),
        queue,
        events: events.clone(),
    };

    tokio::spawn(worker_loop(
        receiver,
        WorkerState {
            tree,
            jobs,
            events,
        },
    ));

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/healthz", get(healthz))
        .route("/api/tree", get(get_tree).put(replace_tree))
        .route("/api/nodes", post(create_node))
        .route("/api/nodes/{id}", patch(update_node).delete(delete_node))
        .route("/api/nodes/delete", post(delete_nodes))
        .route("/api/nodes/move", post(move_nodes))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/jobs/events", get(job_events))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers(Any),
        )
        .with_state(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3001));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("file explorer backend listening on http://{addr}");
    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Clone)]
struct AppState {
    tree: Arc<RwLock<TreeSnapshot>>,
    jobs: Arc<RwLock<HashMap<JobId, JobRecord>>>,
    queue: mpsc::Sender<QueuedJob>,
    events: broadcast::Sender<JobEvent>,
}

#[derive(Clone)]
struct WorkerState {
    tree: Arc<RwLock<TreeSnapshot>>,
    jobs: Arc<RwLock<HashMap<JobId, JobRecord>>>,
    events: broadcast::Sender<JobEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TreeSnapshot {
    schema: String,
    version: u64,
    root: FileNode,
}

impl TreeSnapshot {
    fn demo() -> Self {
        let mut snapshot = Self {
            schema: SCHEMA.into(),
            version: 1,
            root: FileNode {
                id: "root".into(),
                name: "Workspace".into(),
                kind: NodeKind::Folder,
                size: 0,
                modified: now_iso(),
                mime: None,
                meta: BTreeMap::from([("owner".into(), json!("axum-demo"))]),
                actions: default_actions(NodeKind::Folder),
                children: vec![
                    FileNode {
                        id: "src".into(),
                        name: "src".into(),
                        kind: NodeKind::Folder,
                        size: 0,
                        modified: now_iso(),
                        mime: None,
                        meta: BTreeMap::new(),
                        actions: default_actions(NodeKind::Folder),
                        children: vec![
                            FileNode {
                                id: "src-main".into(),
                                name: "main.rs".into(),
                                kind: NodeKind::File,
                                size: 18_420,
                                modified: now_iso(),
                                mime: Some("text/rust".into()),
                                meta: BTreeMap::new(),
                                actions: default_actions(NodeKind::File),
                                children: Vec::new(),
                            },
                            FileNode {
                                id: "src-ui".into(),
                                name: "explorer.ts".into(),
                                kind: NodeKind::File,
                                size: 41_811,
                                modified: now_iso(),
                                mime: Some("text/typescript".into()),
                                meta: BTreeMap::new(),
                                actions: default_actions(NodeKind::File),
                                children: Vec::new(),
                            },
                        ],
                    },
                    FileNode {
                        id: "docs".into(),
                        name: "docs".into(),
                        kind: NodeKind::Folder,
                        size: 0,
                        modified: now_iso(),
                        mime: None,
                        meta: BTreeMap::new(),
                        actions: default_actions(NodeKind::Folder),
                        children: vec![FileNode {
                            id: "docs-contract".into(),
                            name: "api-contract.md".into(),
                            kind: NodeKind::File,
                            size: 12_104,
                            modified: now_iso(),
                            mime: Some("text/markdown".into()),
                            meta: BTreeMap::new(),
                            actions: default_actions(NodeKind::File),
                            children: Vec::new(),
                        }],
                    },
                ],
            },
        };
        recompute_sizes(&mut snapshot.root);
        snapshot
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileNode {
    id: NodeId,
    name: String,
    kind: NodeKind,
    #[serde(default)]
    size: u64,
    #[serde(default = "now_iso")]
    modified: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mime: Option<String>,
    #[serde(default)]
    meta: BTreeMap<String, Value>,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    children: Vec<FileNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NodeKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobRecord {
    id: JobId,
    operation: JobOperation,
    status: JobStatus,
    progress: u8,
    description: String,
    affected_node_ids: Vec<NodeId>,
    created_at_ms: u64,
    updated_at_ms: u64,
    result: Option<JobResult>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
enum JobOperation {
    TreeReplace(ReplaceTreeRequest),
    NodeCreate(CreateNodeRequest),
    NodeUpdate { id: NodeId, patch: UpdateNodeRequest },
    NodeDelete(DeleteNodesRequest),
    MultiMove(MultiMoveRequest),
}

impl JobOperation {
    fn event_name(&self) -> &'static str {
        match self {
            JobOperation::TreeReplace(_) => "tree_replace",
            JobOperation::NodeCreate(_) => "node_create",
            JobOperation::NodeUpdate { .. } => "node_update",
            JobOperation::NodeDelete(_) => "node_delete",
            JobOperation::MultiMove(_) => "multi_move",
        }
    }

    fn label(&self) -> String {
        match self {
            JobOperation::TreeReplace(_) => "Replace tree".into(),
            JobOperation::NodeCreate(request) => format!("Create {}", request.name),
            JobOperation::NodeUpdate { id, .. } => format!("Update {id}"),
            JobOperation::NodeDelete(request) => {
                format!("Delete {} item{}", request.ids.len(), plural(request.ids.len()))
            }
            JobOperation::MultiMove(request) => {
                let count = request.normalized_moves().len();
                format!("Move {count} item{}", plural(count))
            }
        }
    }

    fn affected_node_ids(&self) -> Vec<NodeId> {
        match self {
            JobOperation::TreeReplace(_) => vec!["root".into()],
            JobOperation::NodeCreate(request) => vec![request.parent_id.clone()],
            JobOperation::NodeUpdate { id, .. } => vec![id.clone()],
            JobOperation::NodeDelete(request) => request.ids.clone(),
            JobOperation::MultiMove(request) => request
                .normalized_moves()
                .into_iter()
                .map(|item| item.node_id)
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
struct QueuedJob {
    id: JobId,
    operation: JobOperation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum JobResult {
    Tree { root: FileNode, version: u64 },
    Node { node: FileNode, version: u64 },
    Deleted { node_ids: Vec<NodeId>, version: u64 },
    Moved { node_ids: Vec<NodeId>, version: u64 },
    Replaced { root: FileNode, version: u64 },
}

#[derive(Debug, Clone, Serialize)]
struct JobEvent {
    event: &'static str,
    job: JobRecord,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedJob {
    job_id: JobId,
    label: String,
    status_url: String,
    events_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TreeQuery {
    #[serde(default)]
    root_id: Option<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaceTreeRequest {
    root: FileNode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateNodeRequest {
    parent_id: NodeId,
    #[serde(default)]
    id: Option<NodeId>,
    name: String,
    kind: NodeKind,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    modified: Option<String>,
    #[serde(default)]
    mime: Option<String>,
    #[serde(default)]
    meta: BTreeMap<String, Value>,
    #[serde(default)]
    actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateNodeRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    modified: Option<String>,
    #[serde(default)]
    mime: Option<String>,
    #[serde(default)]
    meta: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    actions: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteNodesRequest {
    ids: Vec<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MultiMoveRequest {
    #[serde(default)]
    moves: Vec<MoveInstruction>,
    #[serde(default)]
    ids: Vec<NodeId>,
    #[serde(default)]
    target_parent_id: Option<NodeId>,
    #[serde(default)]
    position: Option<usize>,
}

impl MultiMoveRequest {
    fn normalized_moves(&self) -> Vec<MoveInstruction> {
        if !self.moves.is_empty() {
            return self.moves.clone();
        }

        let Some(new_parent_id) = self.target_parent_id.clone() else {
            return Vec::new();
        };

        self.ids
            .iter()
            .enumerate()
            .map(|(offset, node_id)| MoveInstruction {
                node_id: node_id.clone(),
                new_parent_id: new_parent_id.clone(),
                index: self.position.map(|position| position + offset),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveInstruction {
    node_id: NodeId,
    new_parent_id: NodeId,
    #[serde(default)]
    index: Option<usize>,
}

async fn healthz() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn get_tree(
    State(state): State<AppState>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<TreeSnapshot>, ApiError> {
    let tree = state.tree.read().await;
    let Some(root_id) = query.root_id else {
        return Ok(Json(tree.clone()));
    };

    let root = find_node(&tree.root, &root_id)
        .ok_or_else(|| ApiError::not_found(format!("node '{root_id}' was not found")))?;

    Ok(Json(TreeSnapshot {
        schema: tree.schema.clone(),
        version: tree.version,
        root,
    }))
}

async fn replace_tree(
    State(state): State<AppState>,
    Json(request): Json<ReplaceTreeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    enqueue_job(state, JobOperation::TreeReplace(request)).await
}

async fn create_node(
    State(state): State<AppState>,
    Json(request): Json<CreateNodeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    enqueue_job(state, JobOperation::NodeCreate(request)).await
}

async fn update_node(
    State(state): State<AppState>,
    Path(id): Path<NodeId>,
    Json(patch): Json<UpdateNodeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    enqueue_job(state, JobOperation::NodeUpdate { id, patch }).await
}

async fn delete_node(
    State(state): State<AppState>,
    Path(id): Path<NodeId>,
) -> Result<impl IntoResponse, ApiError> {
    enqueue_job(state, JobOperation::NodeDelete(DeleteNodesRequest { ids: vec![id] })).await
}

async fn delete_nodes(
    State(state): State<AppState>,
    Json(request): Json<DeleteNodesRequest>,
) -> Result<impl IntoResponse, ApiError> {
    enqueue_job(state, JobOperation::NodeDelete(request)).await
}

async fn move_nodes(
    State(state): State<AppState>,
    Json(request): Json<MultiMoveRequest>,
) -> Result<impl IntoResponse, ApiError> {
    enqueue_job(state, JobOperation::MultiMove(request)).await
}

async fn get_job(
    State(state): State<AppState>,
    Path(id): Path<JobId>,
) -> Result<Json<JobRecord>, ApiError> {
    state
        .jobs
        .read()
        .await
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found("job not found"))
}

async fn job_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();

    let stream = stream! {
        loop {
            match receiver.recv().await {
                Ok(job_event) => {
                    let event_name = job_event.event;
                    let payload = serde_json::to_string(&job_event)
                        .unwrap_or_else(|_| r#"{"error":"event serialization failed"}"#.into());
                    yield Ok(Event::default().event(event_name).data(payload));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn enqueue_job(
    state: AppState,
    operation: JobOperation,
) -> Result<(StatusCode, Json<AcceptedJob>), ApiError> {
    let id = Uuid::new_v4();
    let now = now_ms();
    let record = JobRecord {
        id,
        operation: operation.clone(),
        status: JobStatus::Queued,
        progress: 0,
        description: "Queued".into(),
        affected_node_ids: operation.affected_node_ids(),
        created_at_ms: now,
        updated_at_ms: now,
        result: None,
        error: None,
    };

    state.jobs.write().await.insert(id, record.clone());
    broadcast_job_event(&state.events, record);

    state
        .queue
        .send(QueuedJob { id, operation: operation.clone() })
        .await
        .map_err(|_| ApiError::service_unavailable("job queue is closed"))?;

    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedJob {
            job_id: id,
            label: operation.label(),
            status_url: format!("/api/jobs/{id}"),
            events_url: "/api/jobs/events".into(),
        }),
    ))
}

async fn worker_loop(mut receiver: mpsc::Receiver<QueuedJob>, state: WorkerState) {
    while let Some(job) = receiver.recv().await {
        update_job(&state, job.id, JobStatus::Running, 8, "Accepted by worker", None).await;
        sleep(Duration::from_millis(140)).await;
        update_job(&state, job.id, JobStatus::Running, 38, "Validating operation", None).await;
        sleep(Duration::from_millis(180)).await;

        let result = run_job(&state.tree, job.operation).await;

        match result {
            Ok(job_result) => {
                update_job(&state, job.id, JobStatus::Running, 82, "Committing tree version", None)
                    .await;
                sleep(Duration::from_millis(120)).await;
                update_job(
                    &state,
                    job.id,
                    JobStatus::Succeeded,
                    100,
                    "Committed",
                    Some(Ok(job_result)),
                )
                .await;
            }
            Err(error) => {
                update_job(
                    &state,
                    job.id,
                    JobStatus::Failed,
                    100,
                    "Failed",
                    Some(Err(error)),
                )
                .await;
            }
        }
    }
}

async fn update_job(
    state: &WorkerState,
    id: JobId,
    status: JobStatus,
    progress: u8,
    description: impl Into<String>,
    terminal: Option<Result<JobResult, String>>,
) {
    let mut jobs = state.jobs.write().await;
    let Some(record) = jobs.get_mut(&id) else {
        return;
    };

    record.status = status;
    record.progress = progress;
    record.description = description.into();
    record.updated_at_ms = now_ms();

    if let Some(result) = terminal {
        match result {
            Ok(job_result) => {
                record.result = Some(job_result);
                record.error = None;
            }
            Err(error) => {
                record.result = None;
                record.error = Some(error);
            }
        }
    }

    broadcast_job_event(&state.events, record.clone());
}

async fn run_job(
    tree: &Arc<RwLock<TreeSnapshot>>,
    operation: JobOperation,
) -> Result<JobResult, String> {
    match operation {
        JobOperation::TreeReplace(request) => replace_tree_job(tree, request).await,
        JobOperation::NodeCreate(request) => create_node_job(tree, request).await,
        JobOperation::NodeUpdate { id, patch } => update_node_job(tree, id, patch).await,
        JobOperation::NodeDelete(request) => delete_nodes_job(tree, request).await,
        JobOperation::MultiMove(request) => multi_move_job(tree, request).await,
    }
}

async fn replace_tree_job(
    tree: &Arc<RwLock<TreeSnapshot>>,
    request: ReplaceTreeRequest,
) -> Result<JobResult, String> {
    let mut root = request.root;
    normalize_tree(&mut root)?;
    recompute_sizes(&mut root);

    let mut tree = tree.write().await;
    tree.version += 1;
    tree.schema = SCHEMA.into();
    tree.root = root.clone();

    Ok(JobResult::Replaced {
        root,
        version: tree.version,
    })
}

async fn create_node_job(
    tree: &Arc<RwLock<TreeSnapshot>>,
    request: CreateNodeRequest,
) -> Result<JobResult, String> {
    validate_name(&request.name)?;

    let mut tree = tree.write().await;
    let mut node = FileNode {
        id: request.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        name: request.name,
        kind: request.kind,
        size: request.size,
        modified: request.modified.unwrap_or_else(now_iso),
        mime: request.mime,
        meta: request.meta,
        actions: request.actions,
        children: Vec::new(),
    };

    normalize_tree(&mut node)?;

    if find_node(&tree.root, &node.id).is_some() {
        return Err(format!("node id '{}' already exists", node.id));
    }

    insert_child(&mut tree.root, &request.parent_id, node.clone(), None)?;
    recompute_sizes(&mut tree.root);
    tree.version += 1;

    Ok(JobResult::Node {
        node,
        version: tree.version,
    })
}

async fn update_node_job(
    tree: &Arc<RwLock<TreeSnapshot>>,
    id: NodeId,
    patch: UpdateNodeRequest,
) -> Result<JobResult, String> {
    if patch.name.is_none()
        && patch.size.is_none()
        && patch.modified.is_none()
        && patch.mime.is_none()
        && patch.meta.is_none()
        && patch.actions.is_none()
    {
        return Err("update request did not include any changes".into());
    }

    if let Some(name) = patch.name.as_deref() {
        validate_name(name)?;
    }

    let mut tree = tree.write().await;
    if let Some(name) = patch.name.as_deref() {
        if let Some(parent_id) = find_parent_id(&tree.root, &id) {
            let parent = find_node(&tree.root, &parent_id)
                .ok_or_else(|| format!("parent '{parent_id}' was not found"))?;
            if parent
                .children
                .iter()
                .any(|child| child.id != id && child.name.eq_ignore_ascii_case(name))
            {
                return Err(format!(
                    "parent '{parent_id}' already contains an item named '{name}'"
                ));
            }
        }
    }

    let updated = {
        let node = find_node_mut(&mut tree.root, &id)
            .ok_or_else(|| format!("node '{id}' was not found"))?;

        if let Some(name) = patch.name {
            node.name = name;
            node.modified = now_iso();
        }

        if let Some(size) = patch.size {
            node.size = size;
            node.modified = now_iso();
        }

        if let Some(modified) = patch.modified {
            node.modified = modified;
        }

        if let Some(mime) = patch.mime {
            node.mime = Some(mime);
        }

        if let Some(meta) = patch.meta {
            node.meta = meta;
        }

        if let Some(actions) = patch.actions {
            node.actions = actions;
        }

        normalize_node_shape(node);
        node.clone()
    };

    recompute_sizes(&mut tree.root);
    tree.version += 1;

    Ok(JobResult::Node {
        node: updated,
        version: tree.version,
    })
}

async fn delete_nodes_job(
    tree: &Arc<RwLock<TreeSnapshot>>,
    request: DeleteNodesRequest,
) -> Result<JobResult, String> {
    if request.ids.is_empty() {
        return Err("delete request must include at least one node id".into());
    }

    let ids = unique_ids(request.ids);
    let mut tree = tree.write().await;
    let mut next_root = tree.root.clone();

    for id in &ids {
        if next_root.id == *id {
            return Err("root node cannot be deleted".into());
        }
        if find_node(&next_root, id).is_none() {
            return Err(format!("node '{id}' was not found"));
        }
    }

    for id in &ids {
        let _ = remove_node(&mut next_root, id);
    }

    recompute_sizes(&mut next_root);
    tree.version += 1;
    tree.root = next_root;

    Ok(JobResult::Deleted {
        node_ids: ids,
        version: tree.version,
    })
}

async fn multi_move_job(
    tree: &Arc<RwLock<TreeSnapshot>>,
    request: MultiMoveRequest,
) -> Result<JobResult, String> {
    let moves = request.normalized_moves();
    if moves.is_empty() {
        return Err("move request must include moves or ids plus targetParentId".into());
    }

    let moved_ids = unique_ids(moves.iter().map(|item| item.node_id.clone()).collect());
    if moved_ids.len() != moves.len() {
        return Err("a node cannot appear in more than one move".into());
    }

    let moved_id_set: HashSet<_> = moved_ids.iter().cloned().collect();
    let mut tree = tree.write().await;
    let mut next_root = tree.root.clone();

    for item in &moves {
        validate_move(&next_root, item, &moved_id_set)?;
    }

    let mut removed = Vec::with_capacity(moves.len());
    for item in &moves {
        let node = remove_node(&mut next_root, &item.node_id)
            .ok_or_else(|| format!("node '{}' was not found", item.node_id))?;
        removed.push((item, node));
    }

    for (item, node) in removed {
        insert_child(&mut next_root, &item.new_parent_id, node, item.index)?;
    }

    recompute_sizes(&mut next_root);
    tree.version += 1;
    tree.root = next_root;

    Ok(JobResult::Moved {
        node_ids: moved_ids,
        version: tree.version,
    })
}

fn normalize_tree(root: &mut FileNode) -> Result<(), String> {
    let mut seen = HashSet::new();
    normalize_node(root, &mut seen)
}

fn normalize_node(node: &mut FileNode, seen: &mut HashSet<NodeId>) -> Result<(), String> {
    if node.id.trim().is_empty() {
        return Err("all nodes must have an id".into());
    }

    if !seen.insert(node.id.clone()) {
        return Err(format!("duplicate node id '{}'", node.id));
    }

    validate_name(&node.name)?;
    normalize_node_shape(node);

    if node.kind == NodeKind::Folder {
        for child in &mut node.children {
            normalize_node(child, seen)?;
        }
    }

    Ok(())
}

fn normalize_node_shape(node: &mut FileNode) {
    if node.modified.trim().is_empty() {
        node.modified = now_iso();
    }

    if node.actions.is_empty() {
        node.actions = default_actions(node.kind);
    }

    if node.kind == NodeKind::File {
        node.children.clear();
    }
}

fn default_actions(kind: NodeKind) -> Vec<String> {
    match kind {
        NodeKind::File => ["open", "rename", "move", "delete"]
            .into_iter()
            .map(String::from)
            .collect(),
        NodeKind::Folder => ["open", "new-file", "new-folder", "rename", "move", "delete"]
            .into_iter()
            .map(String::from)
            .collect(),
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("node name cannot be empty".into());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("node name cannot contain path separators".into());
    }
    Ok(())
}

fn validate_move(
    root: &FileNode,
    item: &MoveInstruction,
    moved_ids: &HashSet<NodeId>,
) -> Result<(), String> {
    if root.id == item.node_id {
        return Err("root node cannot be moved".into());
    }

    if item.node_id == item.new_parent_id {
        return Err(format!("node '{}' cannot be moved into itself", item.node_id));
    }

    if moved_ids.contains(&item.new_parent_id) {
        return Err(format!(
            "destination '{}' is also being moved; move destinations must be stable",
            item.new_parent_id
        ));
    }

    let node = find_node(root, &item.node_id)
        .ok_or_else(|| format!("node '{}' was not found", item.node_id))?;
    let parent = find_node(root, &item.new_parent_id)
        .ok_or_else(|| format!("destination '{}' was not found", item.new_parent_id))?;

    if parent.kind != NodeKind::Folder {
        return Err(format!("destination '{}' is not a folder", item.new_parent_id));
    }

    if node.kind == NodeKind::Folder && is_descendant(root, &item.node_id, &item.new_parent_id) {
        return Err(format!(
            "folder '{}' cannot be moved into one of its descendants",
            item.node_id
        ));
    }

    Ok(())
}

fn find_node(root: &FileNode, id: &str) -> Option<FileNode> {
    if root.id == id {
        return Some(root.clone());
    }

    root.children.iter().find_map(|child| find_node(child, id))
}

fn find_node_mut<'a>(root: &'a mut FileNode, id: &str) -> Option<&'a mut FileNode> {
    if root.id == id {
        return Some(root);
    }

    for child in &mut root.children {
        if let Some(found) = find_node_mut(child, id) {
            return Some(found);
        }
    }

    None
}

fn find_parent_id(root: &FileNode, id: &str) -> Option<NodeId> {
    for child in &root.children {
        if child.id == id {
            return Some(root.id.clone());
        }

        if let Some(parent_id) = find_parent_id(child, id) {
            return Some(parent_id);
        }
    }

    None
}

fn insert_child(
    root: &mut FileNode,
    parent_id: &str,
    node: FileNode,
    index: Option<usize>,
) -> Result<(), String> {
    let parent = find_node_mut(root, parent_id)
        .ok_or_else(|| format!("parent '{parent_id}' was not found"))?;

    if parent.kind != NodeKind::Folder {
        return Err(format!("parent '{parent_id}' is not a folder"));
    }

    if parent.children.iter().any(|child| child.name == node.name) {
        return Err(format!(
            "parent '{parent_id}' already contains an item named '{}'",
            node.name
        ));
    }

    match index {
        Some(index) if index <= parent.children.len() => parent.children.insert(index, node),
        Some(index) => {
            return Err(format!(
                "index {index} is out of bounds for parent '{parent_id}'"
            ))
        }
        None => parent.children.push(node),
    }

    Ok(())
}

fn remove_node(root: &mut FileNode, id: &str) -> Option<FileNode> {
    if let Some(index) = root.children.iter().position(|child| child.id == id) {
        return Some(root.children.remove(index));
    }

    for child in &mut root.children {
        if let Some(node) = remove_node(child, id) {
            return Some(node);
        }
    }

    None
}

fn is_descendant(root: &FileNode, ancestor_id: &str, candidate_id: &str) -> bool {
    let Some(ancestor) = find_node(root, ancestor_id) else {
        return false;
    };

    contains_node_below(&ancestor, candidate_id)
}

fn contains_node_below(node: &FileNode, candidate_id: &str) -> bool {
    node.children
        .iter()
        .any(|child| child.id == candidate_id || contains_node_below(child, candidate_id))
}

fn recompute_sizes(node: &mut FileNode) -> u64 {
    if node.kind == NodeKind::File {
        return node.size;
    }

    node.size = node.children.iter_mut().map(recompute_sizes).sum();
    node.size
}

fn unique_ids(ids: Vec<NodeId>) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    ids.into_iter().filter(|id| seen.insert(id.clone())).collect()
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn broadcast_job_event(sender: &broadcast::Sender<JobEvent>, job: JobRecord) {
    let _ = sender.send(JobEvent {
        event: job.operation.event_name(),
        job,
    });
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_iso() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| now_ms().to_string())
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "message": self.message,
                }
            })),
        )
            .into_response()
    }
}
