"""Private local processes for a small Sift trial. No cloud resources are changed."""
import http.client, json, os, re, secrets, socket, ssl, subprocess, threading, time, uuid
from contextlib import closing
from pathlib import Path
from load_client import Api
from pilot_receiver import server
from run_load import require, write

RECEIVER_LABEL = 'com.faberline.sift.pilot.receiver'


def private(path, data):
    with Path(path).open('xb') as out:
        os.chmod(path, 0o600); out.write(data); out.flush(); os.fsync(out.fileno())


def available_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


class Receiver:
    def __init__(self, root):
        self.root, self.token = Path(root), secrets.token_urlsafe(32)
        self.container, self.server, self.thread = None, None, None

    def start(self, port=443, docker_image=None):
        self.root.mkdir(mode=0o700)
        cert, ca_key = self.root / 'ca.pem', self.root / 'ca-key.pem'
        key, csr, leaf = self.root / 'key.pem', self.root / 'server.csr', self.root / 'server.pem'
        subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
            '-subj', '/CN=Sift pilot root', '-addext', 'basicConstraints=critical,CA:TRUE',
            '-addext', 'keyUsage=critical,keyCertSign,cRLSign', '-addext', 'subjectKeyIdentifier=hash',
            '-keyout', str(ca_key), '-out', str(cert)],
            check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
        subprocess.run(['openssl', 'req', '-new', '-newkey', 'rsa:2048', '-nodes',
            '-subj', '/CN=localhost', '-keyout', str(key), '-out', str(csr)],
            check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
        extensions = self.root / 'extensions.ini'
        private(extensions, b'subjectAltName=DNS:localhost,IP:127.0.0.1\n'
            b'basicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\n'
            b'keyUsage=digitalSignature,keyEncipherment\nauthorityKeyIdentifier=keyid:always,issuer\n')
        subprocess.run(['openssl', 'x509', '-req', '-in', str(csr), '-CA', str(cert), '-CAkey', str(ca_key),
            '-CAcreateserial', '-out', str(leaf), '-days', '1', '-extfile', str(extensions)],
            check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
        for path in (ca_key, key, cert, csr, leaf): path.chmod(0o600)
        self.cert = cert
        private(self.root / 'token', self.token.encode())
        if docker_image:
            require(re.fullmatch(r'(?:sha256:|[a-z0-9][a-z0-9/_.:-]*@sha256:)[0-9a-f]{64}', docker_image),
                'receiver image must use an immutable local ID or registry digest')
            script = Path(__file__).with_name('pilot_receiver.py').resolve()
            # Record ownership before starting Docker so startup errors can still be cleaned up.
            self.container = 'sift-pilot-receiver-' + uuid.uuid4().hex
            subprocess.run(['docker', 'run', '--detach', '--rm', '--pull=never',
                '--name', self.container, '--label', RECEIVER_LABEL + '=' + self.container,
                '--user', str(os.getuid()) + ':' + str(os.getgid()), '--read-only', '--cap-drop=ALL',
                '--security-opt=no-new-privileges', '--memory=128m', '--cpus=.2',
                '--publish', '127.0.0.1:' + str(port) + ':8443',
                '--mount', 'type=bind,src=' + str(self.root.resolve()) + ',dst=/state',
                '--mount', 'type=bind,src=' + str(script) + ',dst=/receiver.py,readonly',
                '--entrypoint', 'python3', docker_image, '/receiver.py', '--root', '/state'],
                check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
            until = time.monotonic() + 30
            context = ssl.create_default_context(cafile=str(cert))
            while time.monotonic() < until:
                try:
                    with closing(http.client.HTTPSConnection('localhost', port, timeout=1,
                            context=context)) as connection:
                        connection.request('GET', '/readyz'); response = connection.getresponse()
                        response.read()
                        if response.status == 200: break
                except OSError: pass
                time.sleep(.1)
            else: raise ValueError('Docker HTTPS receiver did not become ready')
        else:
            self.server = server(self.root, port, host='127.0.0.1')
            self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
            self.thread.start()

    def snapshot(self):
        path = self.root / 'deliveries.jsonl'
        if not path.exists(): return []
        raw = path.read_bytes()
        # Readers can see a write before its final newline. Do not parse that tail.
        return [json.loads(line) for line in raw[:raw.rfind(b'\n') + 1].splitlines()]

    def close(self):
        if self.server is not None:
            try:
                if self.thread is not None and self.thread.is_alive():
                    self.server.shutdown()
            finally:
                self.server.server_close()
            if self.thread is not None and self.thread.is_alive():
                self.thread.join(timeout=5)
                require(not self.thread.is_alive(), 'local HTTPS receiver thread did not stop')
            self.server, self.thread = None, None
        if self.container is not None:
            for attempt in range(3):
                try:
                    identity = self.container_id()
                    if identity is None:
                        self.container = None
                        return
                    if attempt < 2:
                        subprocess.run(['docker', 'rm', '--force', identity], check=True,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
                except (OSError, subprocess.SubprocessError):
                    if attempt == 2:
                        raise
            raise ValueError('Docker HTTPS receiver still exists after cleanup: ' + self.container)

    def container_id(self):
        inventory = subprocess.check_output(['docker', 'ps', '--all', '--no-trunc',
            '--filter', 'label=' + RECEIVER_LABEL + '=' + self.container,
            '--format', '{{.ID}} {{.Names}}'], text=True, stderr=subprocess.DEVNULL, timeout=10).strip()
        if not inventory:
            return None
        found = re.fullmatch(r'([0-9a-f]{64}) ' + re.escape(self.container), inventory)
        require(found is not None, 'Docker HTTPS receiver ownership is ambiguous: ' + self.container)
        return found[1]


class LocalSift:
    def __init__(self, binary, root, project, receiver):
        self.binary, self.root, self.project = Path(binary).resolve(), Path(root), project
        self.root.mkdir(mode=0o700)
        self.port, self.grpc = available_port(), available_port()
        self.endpoint = 'http://127.0.0.1:' + str(self.port)
        self.env = {key: value for key, value in os.environ.items() if not key.startswith('SIFT_')}
        token = secrets.token_urlsafe(32)
        self.token_file = self.root / 'admin-token'; private(self.token_file, token.encode())
        registry = self.root / 'tokens.json'
        self.collector_token = secrets.token_urlsafe(32)
        write(registry, {'tokens': {
            token: {'subject': 'pilot-controller', 'roles': {project: 'admin'}},
            self.collector_token: {'subject': 'pilot-collector', 'roles': {project: 'write'}}}})
        secret_dir = self.root / 'webhooks'; secret_dir.mkdir(mode=0o700)
        write(secret_dir / 'pilot.json', {'url': 'https://localhost/notifications', 'bearer_token': receiver.token})
        self.env.update({'SIFT_AUTH': 'required', 'SIFT_TOKEN_REGISTRY_FILE': str(registry),
            'SIFT_TOKEN': token,
            'SIFT_CONTROL_PROJECTS': project, 'SIFT_WEBHOOK_SECRET_DIRECTORY': str(secret_dir),
            'SIFT_WEBHOOK_ALLOWED_HOSTS': 'localhost', 'SSL_CERT_FILE': str(receiver.cert)})
        self.process, self.log = None, None

    def start(self):
        require(self.process is None, 'Sift is already running')
        self.log = (self.root / 'server.log').open('ab'); os.chmod(self.log.name, 0o600)
        self.process = subprocess.Popen([str(self.binary), 'serve', '--host', '127.0.0.1',
            '--port', str(self.port), '--grpc-port', str(self.grpc), '--data-dir', str(self.root / 'data'),
            '--log-level', 'warn'], env=self.env, stdout=self.log, stderr=self.log)
        until, last_status = time.monotonic() + 30, None
        while time.monotonic() < until:
            require(self.process.poll() is None, 'Sift exited before readiness; inspect the private server log')
            try:
                with closing(http.client.HTTPConnection('127.0.0.1', self.port, timeout=1)) as connection:
                    connection.request('GET', '/readyz'); response = connection.getresponse()
                    last_status = response.status
                    response.read(16_385)
                    if last_status == 200: return
            except OSError: pass
            time.sleep(.1)
        raise ValueError('Sift readiness timed out with HTTP ' + str(last_status))

    def stop(self):
        try:
            if self.process:
                self.process.terminate()
                try: self.process.wait(timeout=25)
                except subprocess.TimeoutExpired: self.process.kill(); self.process.wait(timeout=5)
                self.process = None
        finally:
            if self.log: self.log.close(); self.log = None

    def collect(self, source, state, source_id, retries=2):
        env = dict(self.env, SIFT_TOKEN=self.collector_token)
        result = subprocess.run([str(self.binary), 'collect', '--source', str(source), '--source-id', source_id,
            '--data-dir', str(state), '--endpoint', self.endpoint, '--project', self.project,
            '--environment', 'pilot', '--format', 'json', '--max-retries', str(retries),
            '--request-timeout-secs', '2'], env=env, capture_output=True, timeout=30)
        write(self.root / ('collector-' + str(time.time_ns()) + '.json'), {'returncode': result.returncode,
            'stdout': result.stdout.decode(errors='replace'), 'stderr': result.stderr.decode(errors='replace')})
        return result
