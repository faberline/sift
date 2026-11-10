"""A private HTTPS trial receiver. The first valid delivery gets HTTP 503."""
import argparse, hashlib, http.server, json, os, re, ssl, threading, time
from pathlib import Path


def server(root, port, host='0.0.0.0'):
    root = Path(root); token = (root / 'token').read_text().strip()
    lock = threading.Lock()
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_): pass
        def do_GET(self):
            if self.path != '/readyz': self.send_error(404); return
            self.send_response(200); self.send_header('Content-Length', '0'); self.end_headers()
        def do_POST(self):
            try:
                if self.path != '/notifications' or self.headers.get('Authorization') != 'Bearer ' + token:
                    self.send_error(403); return
                size = int(self.headers.get('Content-Length', '0'))
                if not 0 < size <= 65_536: self.send_error(413); return
                raw = self.rfile.read(size); payload = json.loads(raw)
                identity = payload['notification_id']
                if not isinstance(identity, str) or not re.fullmatch('[0-9a-f]{64}', identity) or \
                        identity != self.headers.get('X-Sift-Notification-Id') or identity != self.headers.get('Idempotency-Key'):
                    self.send_error(400); return
                with lock:
                    path = root / 'deliveries.jsonl'
                    status = 503 if not path.exists() else 200
                    record = {'received_at': time.time(), 'status': status,
                        'payload_sha256': hashlib.sha256(raw).hexdigest(), 'payload': payload}
                    with path.open('ab') as out:
                        os.chmod(path, 0o600)
                        out.write((json.dumps(record, sort_keys=True) + '\n').encode())
                        out.flush(); os.fsync(out.fileno())
                self.send_response(status); self.send_header('Content-Length', '0'); self.end_headers()
            except (ValueError, KeyError, TypeError): self.send_error(400)
    result = http.server.ThreadingHTTPServer((host, port), Handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(root / 'server.pem', root / 'key.pem')
    result.socket = context.wrap_socket(result.socket, server_side=True)
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True); parser.add_argument('--port', type=int, default=8443)
    args = parser.parse_args()
    server(args.root, args.port).serve_forever()
