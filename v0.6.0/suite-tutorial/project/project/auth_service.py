#!/usr/bin/env -S python3 -u
"""Authentication service backed by the db service."""

import argparse
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


class AuthHandler(BaseHTTPRequestHandler):
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

    def _db_url(self, path):
        return f"http://localhost:{self.server.db_port}{path}"

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

    def do_POST(self):
        path = self.path.strip("/")
        body = json.loads(self._read_body())
        login = body.get("login", "")
        password = body.get("password", "")

        if path == "register":
            self._handle_register(login, password)
        elif path == "login":
            self._handle_login(login, password)
        else:
            self._send(404, {"error": "not found"})

    def _handle_register(self, login, password):
        # Check if user already exists
        status, data = self._db_get(login)
        if status is None:
            print("error: db unavailable", flush=True)
            self._send(502, {"error": "db unavailable"})
            return
        if status == 200:
            print(f"error: {login} already exists", flush=True)
            self._send(409)
            return

        # Write credentials
        result = self._db_put(login, password)
        if result is None:
            print("error: db unavailable", flush=True)
            self._send(502, {"error": "db unavailable"})
            return

        print(f"registered {login}", flush=True)
        self._send(200)

    def _handle_login(self, login, password):
        status, data = self._db_get(login)
        if status is None:
            print("error: db unavailable", flush=True)
            self._send(502, {"error": "db unavailable"})
            return
        if status == 404:
            print(f"error: {login} not found", flush=True)
            self._send(404)
            return

        stored_password = data.get("value", "")
        if stored_password != password:
            print(f"login denied {login}", flush=True)
            self._send(401)
            return

        print(f"login ok {login}", flush=True)
        self._send(200)


def main():
    parser = argparse.ArgumentParser(description="Authentication service")
    parser.add_argument("--port", type=int, default=9010)
    parser.add_argument("--db-port", type=int, default=9000)
    parser.add_argument("--db-name", default="auth")
    args = parser.parse_args()

    FastHTTPServer.allow_reuse_address = True
    server = FastHTTPServer(("127.0.0.1", args.port), AuthHandler)
    server.db_port = args.db_port
    server.db_name = args.db_name
    print(f"listening on {args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
