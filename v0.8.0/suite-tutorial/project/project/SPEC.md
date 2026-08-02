# Real-World Tutorial: Task Manager Integration Tests

An example Relux project that tests a three-service stack: a key-value database, an
authentication service, and a task manager API. All services are small Python scripts
included in the example.

## Architecture

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ task_service │────▶│ auth_service │     │              │
│  :task_port  │     │  :auth_port  │     │  db_service  │
│              │──┐  └──────┬───────┘     │  :db_port    │
└──────────────┘  │         │             │              │
                  │         └────────────▶│              │
                  └──────────────────────▶│              │
                                          └──────────────┘
```

All three services use JSON request/response bodies.

---

## 1. db_service.py — Key-Value Database

A simple HTTP key-value store with named databases backed by flat files.

### CLI

```
python db_service.py --port PORT --data-dir DIR
```

- `--port` — port to listen on (default: `9000`)
- `--data-dir` — root storage directory, created if absent (default: `/tmp/database`)

### Endpoints

| Method   | Path               | Request Body       | Success Response      | Status |
|----------|--------------------|--------------------|-----------------------|--------|
| `POST`   | `/db/{name}`       | —                  | `{"created": "name"}` | 200    |
| `GET`    | `/db/{name}/{key}` | —                  | `{"value": "..."}`    | 200    |
| `PUT`    | `/db/{name}/{key}` | `{"value": "..."}` | `{"wrote": "key"}`    | 200    |
| `DELETE` | `/db/{name}/{key}` | —                  | `{"deleted": "key"}`  | 200    |

### Error Responses

| Condition         | Body                                     | Status |
|-------------------|------------------------------------------|--------|
| DB already exists | `{"error": "db NAME already exists"}`    | 409    |
| DB not found      | `{"error": "db NAME not found"}`         | 404    |
| Key not found     | `{"error": "key KEY not found in NAME"}` | 404    |

### Logging (stdout, plain text)

**Info:**
- `listening on PORT`
- `created db NAME`
- `read KEY from NAME`
- `wrote KEY to NAME`
- `deleted KEY from NAME`

**Error:**
- `error: db NAME already exists`
- `error: db NAME not found`
- `error: key KEY not found in NAME`

### Storage Layout

```
data-dir/
└── mydb/
    ├── key1    # file content = value
    └── key2
```

---

## 2. auth_service.py — Authentication Service

A simple auth service that stores credentials in the db service. Plain text
login/password pairs, login as key, password as value.

### CLI

```
python auth_service.py --port PORT --db-port DB_PORT --db-name NAME
```

- `--port` — port to listen on (default: `9010`)
- `--db-port` — port of the running db_service (default: `9000`)
- `--db-name` — database name to use for credential storage (default: `auth`)

**Startup:** expects the database to already exist (does not create it).
Prints `listening on PORT` when ready.

### Endpoints

| Method | Path        | Request Body                          | Success Status | Failure Status                   |
|--------|-------------|---------------------------------------|----------------|----------------------------------|
| `POST` | `/register` | `{"login": "...", "password": "..."}` | 200            | 409 (exists)                     |
| `POST` | `/login`    | `{"login": "...", "password": "..."}` | 200            | 401 (wrong pw) / 404 (not found) |

Response bodies are empty. Success or failure is determined by HTTP status code only.

### Logging (stdout, plain text)

**Info:**
- `listening on PORT`
- `registered LOGIN`
- `login ok LOGIN`
- `login denied LOGIN`

**Error:**
- `error: LOGIN already exists`
- `error: LOGIN not found`
- `error: db unavailable`

---

## 3. task_service.py — Task Manager

The main system under test. CRUD API for tasks, authenticated via the auth service,
stored in the db service.

### CLI

```
python task_service.py --port PORT --db-port DB_PORT --auth-port AUTH_PORT --db-name NAME
```

- `--port` — port to listen on (default: `9020`)
- `--db-port` — port of the running db_service (default: `9000`)
- `--auth-port` — port of the running auth_service (default: `9010`)
- `--db-name` — database name for task storage (default: `tasks`)

**Startup:** expects the database to already exist (does not create it).
Prints `listening on PORT` when ready.

### Authentication

The task service has its own `/login` endpoint. It forwards credentials to the auth
service, and on success issues an opaque token (base64-encoded login). All other
endpoints require `Authorization: Bearer TOKEN`.

### Endpoints

| Method   | Path          | Request Body                          | Success Response                                  | Status |
|----------|---------------|---------------------------------------|---------------------------------------------------|--------|
| `POST`   | `/login`      | `{"login": "...", "password": "..."}` | `{"token": "..."}`                                | 200    |
| `POST`   | `/tasks`      | `{"title": "...", "status": "todo"}`  | `{"id": "...", "title": "...", "status": "todo"}` | 200    |
| `GET`    | `/tasks`      | —                                     | `{"tasks": [...]}`                                | 200    |
| `GET`    | `/tasks/{id}` | —                                     | `{"id": "...", "title": "...", "status": "..."}`  | 200    |
| `PUT`    | `/tasks/{id}` | `{"title": "...", "status": "done"}`  | `{"id": "...", "title": "...", "status": "..."}`  | 200    |
| `DELETE` | `/tasks/{id}` | —                                     | `{"deleted": "id"}`                               | 200    |

### Error Responses

| Condition                        | Body                             | Status |
|----------------------------------|----------------------------------|--------|
| Missing/invalid token            | `{"error": "unauthorized"}`      | 401    |
| Login: auth rejects credentials  | `{"error": "forbidden"}`         | 403    |
| Task not found                   | `{"error": "task ID not found"}` | 404    |
| Auth service unavailable         | `{"error": "auth unavailable"}`  | 502    |

### Logging (stdout, plain text)

**Info:**
- `listening on PORT`
- `issued token for LOGIN`
- `created task ID for LOGIN`
- `listed tasks for LOGIN`
- `read task ID for LOGIN`
- `updated task ID for LOGIN`
- `deleted task ID for LOGIN`

**Error:**
- `error: unauthorized`
- `error: forbidden LOGIN`
- `error: task ID not found`
- `error: auth unavailable`

### Storage Scheme

Tasks are stored in the db service under the configured `--db-name` database (default: `tasks`).
Three key patterns per user:

| Key                 | Value                               | Purpose                          |
|---------------------|-------------------------------------|----------------------------------|
| `{login}:__next_id` | `"4"`                               | Next task ID counter             |
| `{login}:__index`   | `"1,2,3"`                           | Comma-separated list of task IDs |
| `{login}:{id}`      | `{"title": "...", "status": "..."}` | Task data                        |

**First task for a new user:** if `{login}:__next_id` returns 404, start from ID `1`.
If `{login}:__index` returns 404, treat as empty list.

**Create flow** (`POST /tasks`):

1. Read `{login}:__next_id` — use `1` if 404
2. Write task to `{login}:{id}`
3. Read `{login}:__index` — use `""` if 404
4. Append new ID to index, write back `{login}:__index`
5. Write incremented `{login}:__next_id`

**List flow** (`GET /tasks`): read `__index`, fetch each `{login}:{id}`.

**Delete flow** (`DELETE /tasks/{id}`): delete `{login}:{id}`, remove ID from `__index`, write back.
