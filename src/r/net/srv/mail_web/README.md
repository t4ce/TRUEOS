# TRUEOS Mail Web

Small standalone static mailbox frontend. It can be served directly from this
folder with any static file server; no Rust service code or build step is
required.

## Files

- `index.html` loads the page shell and Tailwind CSS from the CDN.
- `app.js` handles mailbox list refresh, message detail loading, and compose
  submission.

## Endpoint Contract

`GET /api/mail/list`

```json
{
  "messages": [
    {
      "id": "abc",
      "from": "ada@example.test",
      "subject": "Hello",
      "preview": "Short body preview",
      "date": "2026-05-09T10:30:00Z",
      "unread": true
    }
  ]
}
```

`GET /api/mail/read?id=abc`

```json
{
  "id": "abc",
  "from": "ada@example.test",
  "to": "root@trueos",
  "subject": "Hello",
  "date": "2026-05-09T10:30:00Z",
  "body": "Plain text message body"
}
```

`POST /api/mail/send`

Request:

```json
{
  "to": "ada@example.test",
  "subject": "Re: Hello",
  "body": "Plain text message body"
}
```

Response:

```json
{
  "ok": true,
  "id": "sent-123"
}
```
