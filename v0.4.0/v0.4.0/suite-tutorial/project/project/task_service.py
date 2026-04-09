#!/usr/bin/env -S python3 -u
"""Task manager service backed by db and auth services."""

import argparse
import base64
import json
import urllib.request
import urllib.error
import socketserver
from http.server import HTTPServer, BaseHTTPRequestHandler


class FastHTTPServer(HTTPServer):
    """HTTPServer that skips the slow getfqdn() call in server_bind."""

    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        _host, port = self.server_address[:2]
        self.server_name = "localhost"
        self.server_port = port


class TaskHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def _send(self, status, body=None):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        if body is not None:
            self.wfile.write(json.dumps(body).encode())

    def _read_body(self):
        length = int(self.headers.get("Content-Length", 0))
        return self.rfile.read(length) if length else b""

    def _extract_login(self):
        auth = self.headers.get("Authorization", "")
        if not auth.startswith("Bearer "):
            return None
        token = auth[len("Bearer "):]
        try:
            return base64.b64decode(token).decode()
        except Exception:
            return None

    def _db_url(self, path):
        return f"http://localhost:{self.server.db_port}{path}"

    def _auth_url(self, path):
        return f"http://localhost:{self.server.auth_port}{path}"

    def _db_get(self, key):
        url = self._db_url(f"/db/{self.server.db_name}/{key}")
        try:
            req = urllib.request.Request(url)
            with urllib.request.urlopen(req) as resp:
                return resp.status, json.loads(resp.read())
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read())
        except Exception:
            return None, None

    def _db_put(self, key, value):
        url = self._db_url(f"/db/{self.server.db_name}/{key}")
        data = json.dumps({"value": value}).encode()
        try:
            req = urllib.request.Request(url, data=data, method="PUT")
            req.add_header("Content-Type", "application/json")
            with urllib.request.urlopen(req) as resp:
                return resp.status
        except urllib.error.HTTPError as e:
            return e.code
        except Exception:
            return None

    def _db_delete(self, key):
        url = self._db_url(f"/db/{self.server.db_name}/{key}")
        try:
            req = urllib.request.Request(url, method="DELETE")
            with urllib.request.urlopen(req) as resp:
                return resp.status
        except urllib.error.HTTPError as e:
            return e.code
        except Exception:
            return None

    def _auth_login(self, login, password):
        url = self._auth_url("/login")
        data = json.dumps({"login": login, "password": password}).encode()
        try:
            req = urllib.request.Request(url, data=data, method="POST")
            req.add_header("Content-Type", "application/json")
            with urllib.request.urlopen(req) as resp:
                return resp.status
        except urllib.error.HTTPError as e:
            return e.code
        except Exception:
            return None

    def _parse_path(self):
        parts = self.path.strip("/").split("/")
        if not parts or parts[0] != "tasks":
            return None, None
        if len(parts) == 1:
            return "tasks", None
        return "tasks", parts[1]

    # --- Login endpoint ---

    def _handle_login(self):
        body = json.loads(self._read_body())
        login = body.get("login", "")
        password = body.get("password", "")

        status = self._auth_login(login, password)
        if status is None:
            print("error: auth unavailable", flush=True)
            self._send(502, {"error": "auth unavailable"})
            return
        if status != 200:
            print(f"error: forbidden {login}", flush=True)
            self._send(403, {"error": "forbidden"})
            return

        token = base64.b64encode(login.encode()).decode()
        print(f"issued token for {login}", flush=True)
        self._send(200, {"token": token})

    # --- Task endpoints ---

    def do_POST(self):
        path = self.path.strip("/")
        if path == "login":
            self._handle_login()
            return

        resource, task_id = self._parse_path()
        if resource != "tasks" or task_id is not None:
            self._send(404, {"error": "not found"})
            return

        login = self._extract_login()
        if login is None:
            print("error: unauthorized", flush=True)
            self._send(401, {"error": "unauthorized"})
            return

        body = json.loads(self._read_body())
        title = body.get("title", "")
        status = body.get("status", "todo")

        # Get next ID
        db_status, data = self._db_get(f"{login}:__next_id")
        if db_status is None:
            self._send(502, {"error": "db unavailable"})
            return
        next_id = int(data["value"]) if db_status == 200 else 1

        task_id = str(next_id)
        task = {"id": task_id, "title": title, "status": status}

        # Write task
        self._db_put(f"{login}:{task_id}", json.dumps(task))

        # Update index
        db_status, data = self._db_get(f"{login}:__index")
        if db_status == 200:
            index = data["value"]
            index = f"{index},{task_id}" if index else task_id
        else:
            index = task_id
        self._db_put(f"{login}:__index", index)

        # Increment next_id
        self._db_put(f"{login}:__next_id", str(next_id + 1))

        print(f"created task {task_id} for {login}", flush=True)
        self._send(200, task)

    def do_GET(self):
        resource, task_id = self._parse_path()
        if resource != "tasks":
            self._send(404, {"error": "not found"})
            return

        login = self._extract_login()
        if login is None:
            print("error: unauthorized", flush=True)
            self._send(401, {"error": "unauthorized"})
            return

        if task_id is None:
            self._handle_list_tasks(login)
        else:
            self._handle_get_task(login, task_id)

    def _handle_list_tasks(self, login):
        db_status, data = self._db_get(f"{login}:__index")
        if db_status is None:
            self._send(502, {"error": "db unavailable"})
            return
        if db_status == 404 or data["value"] == "":
            print(f"listed tasks for {login}", flush=True)
            self._send(200, {"tasks": []})
            return

        index = data["value"]
        ids = index.split(",")
        tasks = []
        for tid in ids:
            s, d = self._db_get(f"{login}:{tid}")
            if s == 200:
                tasks.append(json.loads(d["value"]))
        print(f"listed tasks for {login}", flush=True)
        self._send(200, {"tasks": tasks})

    def _handle_get_task(self, login, task_id):
        db_status, data = self._db_get(f"{login}:{task_id}")
        if db_status is None:
            self._send(502, {"error": "db unavailable"})
            return
        if db_status == 404:
            print(f"error: task {task_id} not found", flush=True)
            self._send(404, {"error": f"task {task_id} not found"})
            return
        task = json.loads(data["value"])
        print(f"read task {task_id} for {login}", flush=True)
        self._send(200, task)

    def do_PUT(self):
        resource, task_id = self._parse_path()
        if resource != "tasks" or task_id is None:
            self._send(404, {"error": "not found"})
            return

        login = self._extract_login()
        if login is None:
            print("error: unauthorized", flush=True)
            self._send(401, {"error": "unauthorized"})
            return

        # Check task exists
        db_status, data = self._db_get(f"{login}:{task_id}")
        if db_status is None:
            self._send(502, {"error": "db unavailable"})
            return
        if db_status == 404:
            print(f"error: task {task_id} not found", flush=True)
            self._send(404, {"error": f"task {task_id} not found"})
            return

        existing = json.loads(data["value"])
        body = json.loads(self._read_body())
        existing["title"] = body.get("title", existing["title"])
        existing["status"] = body.get("status", existing["status"])

        self._db_put(f"{login}:{task_id}", json.dumps(existing))
        print(f"updated task {task_id} for {login}", flush=True)
        self._send(200, existing)

    def do_DELETE(self):
        resource, task_id = self._parse_path()
        if resource != "tasks" or task_id is None:
            self._send(404, {"error": "not found"})
            return

        login = self._extract_login()
        if login is None:
            print("error: unauthorized", flush=True)
            self._send(401, {"error": "unauthorized"})
            return

        # Check task exists
        db_status, _ = self._db_get(f"{login}:{task_id}")
        if db_status is None:
            self._send(502, {"error": "db unavailable"})
            return
        if db_status == 404:
            print(f"error: task {task_id} not found", flush=True)
            self._send(404, {"error": f"task {task_id} not found"})
            return

        # Delete task
        self._db_delete(f"{login}:{task_id}")

        # Update index
        db_status, data = self._db_get(f"{login}:__index")
        if db_status == 200:
            ids = data["value"].split(",")
            ids = [i for i in ids if i != task_id]
            self._db_put(f"{login}:__index", ",".join(ids))

        print(f"deleted task {task_id} for {login}", flush=True)
        self._send(200, {"deleted": task_id})


def main():
    parser = argparse.ArgumentParser(description="Task manager service")
    parser.add_argument("--port", type=int, default=9020)
    parser.add_argument("--db-port", type=int, default=9000)
    parser.add_argument("--auth-port", type=int, default=9010)
    parser.add_argument("--db-name", default="tasks")
    args = parser.parse_args()

    FastHTTPServer.allow_reuse_address = True
    server = FastHTTPServer(("127.0.0.1", args.port), TaskHandler)
    server.db_port = args.db_port
    server.auth_port = args.auth_port
    server.db_name = args.db_name
    print(f"listening on {args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
