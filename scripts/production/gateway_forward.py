"""Bind public API traffic to the exact gateway Pod from the runtime check."""
import http.client, socket, subprocess, time

class GatewayForward:
    def __init__(self, kubeconfig, namespace, gateway):
        self.kube = ['kubectl', '--kubeconfig=' + str(kubeconfig), '--request-timeout=30s', '--namespace', namespace]
        self.gateway, self.process = gateway, None

    def __enter__(self):
        # Check the UID before opening the forward. A replaced Pod requires a new run.
        from verify_runtime import command
        pod = command(self.kube + ['get', 'pod', self.gateway['name'], '-o', 'json'])
        if pod['metadata']['uid'] != self.gateway['uid']: raise ValueError('gateway Pod was replaced before the run')
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
        self.process = subprocess.Popen(self.kube + ['port-forward', '--address=127.0.0.1',
            'pod/' + self.gateway['name'], str(port) + ':7380'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 30
        try:
            while time.monotonic() < deadline:
                if self.process.poll() is not None: raise ValueError('gateway port forward stopped')
                try:
                    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=1)
                    connection.request('GET', '/readyz'); response = connection.getresponse()
                    response.read(16384); connection.close()
                    if response.status == 200: return f'http://127.0.0.1:{port}'
                except OSError: pass
                time.sleep(.1)
            raise ValueError('gateway did not become ready through the bound port forward')
        except BaseException:
            self.__exit__(None, None, None)
            raise

    def __exit__(self, kind, value, trace):
        if self.process:
            self.process.terminate()
            try: self.process.wait(timeout=5)
            except subprocess.TimeoutExpired: self.process.kill(); self.process.wait(timeout=5)
