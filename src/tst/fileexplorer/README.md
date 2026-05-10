# FileExplorer Prototype

This repo contains a static single-page file explorer and a Rust/Axum adapter for a hosted backend. The design is job-queue-first: the UI may show pending intent immediately, but it should only apply durable tree changes after the backend job reaches `succeeded`.

## Files

- `index.html`: Tailwind CDN + plain JavaScript prototype with tree navigation, folder tiles, list view, breadcrumbs, inspector, multi-select, drag-and-drop move, rename, context menu, delete/create flows, and a sliding job drawer.
- `backend/`: Axum adapter exposing the same node schema and async job contract.

## Tree Schema

The API uses stable node ids instead of paths. Paths can be derived in the client from parent/child relationships, while ids stay valid through rename and move operations.

```json
{
  "schema": "filetree.v1",
  "version": 7,
  "root": {
    "id": "root",
    "name": "Workspace",
    "kind": "folder",
    "size": 19593274,
    "modified": "2026-05-10T00:00:00Z",
    "mime": null,
    "meta": { "owner": "hosted-demo" },
    "actions": ["open", "new-file", "new-folder", "rename", "move", "delete"],
    "children": []
  }
}
```

`kind` is `"folder"` or `"file"`. Every node carries known `size` information. Folder sizes may be precomputed by the backend or refreshed after successful jobs.

## Async Job Contract

Every mutation returns `202 Accepted`:

```json
{
  "jobId": "2a4f4f7d-2b8f-4ae4-aad1-b67f97f6d82d",
  "label": "Move 3 items",
  "statusUrl": "/api/jobs/2a4f4f7d-2b8f-4ae4-aad1-b67f97f6d82d",
  "eventsUrl": "/api/jobs/events"
}
```

Poll `GET /api/jobs/:id` or subscribe to `GET /api/jobs/events` with `EventSource`. A job record includes `status`, `progress`, `description`, `affectedNodeIds`, `result`, and `error`.

Statuses are `queued`, `running`, `succeeded`, `failed`, and `cancelled`. The client should keep pending badges and avoid success language until `succeeded`.

## Routes

- `GET /api/tree`
- `GET /api/tree?rootId=src`
- `PUT /api/tree` with `{ "root": FileNode }`
- `POST /api/nodes` with `{ "parentId": "root", "name": "notes.md", "kind": "file", "size": 0 }`
- `PATCH /api/nodes/:id` with partial node fields such as `{ "name": "new.md" }`
- `DELETE /api/nodes/:id`
- `POST /api/nodes/delete` with `{ "ids": ["a", "b"] }`
- `POST /api/nodes/move` with `{ "ids": ["a", "b"], "targetParentId": "docs", "position": 0 }`
- `GET /api/jobs/:id`
- `GET /api/jobs/events`

Batch moves can also use explicit instructions:

```json
{
  "moves": [
    { "nodeId": "a", "newParentId": "docs", "index": 0 },
    { "nodeId": "b", "newParentId": "assets" }
  ]
}
```

## Frontend Integration

The page ships with a mock in-memory adapter so it can be opened directly. To attach a hosted backend from the browser console or another script:

```js
const adapter = await FileExplorer.attachBackend("/api");
```

The lower job drawer is intentionally part of the main UX: it shows in-flight work, progress, affected nodes, and terminal state so the interface does not overstate what the backend has actually committed.
