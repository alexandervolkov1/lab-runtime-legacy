"""Finite TIME-1 Windows experiment; Babashka configures the existing Parent API.

The transparent relay records original bytes, creates no requests, and preserves
one Runtime connection. No hardware, GUI commands, or graphics integration.
Optional compatibility argument: old-runtime or old-parent (EFRAME-3 binaries).
"""
import datetime
import json
from pathlib import Path
import select
import socket
import subprocess
import sys
import e2 as common

ROOT = common.ROOT
PROFILE, RENDERER, RUN = sys.argv[1:4]
COMPAT = sys.argv[4] if len(sys.argv) > 4 else 'current'
assert PROFILE in ('dev', 'release') and RENDERER in ('wgpu', 'glow')
assert COMPAT in ('current', 'old-runtime', 'old-parent')
assert RUN.replace('-', '').isalnum()
OUT = ROOT/'target/time-1'/RUN
OUT.mkdir(parents=True, exist_ok=False)
common.OUT = OUT
BIN = 'debug' if PROFILE == 'dev' else 'release'
RUNTIME = ROOT/'target'/BIN/'lab-runtime.exe'
PARENT = ROOT/'target'/BIN/'lab-workbench.exe'
CHILD = ROOT/'scripts/support/eframe-host/target'/BIN/'lab-eframe-host.exe'
if COMPAT == 'old-runtime': RUNTIME = ROOT/'target/time-1/compat-old/lab-runtime.exe'
if COMPAT == 'old-parent': PARENT = ROOT/'target/time-1/compat-old/lab-workbench.exe'


class Relay(common.Relay):
    """Same byte forwarding with bounded two-direction evidence, no active client."""
    def run(self):
        self.hello = None
        try:
            with (OUT/'runtime-client-wire.ndjson').open('ab') as sent, (OUT/'runtime-server-wire.ndjson').open('ab') as received:
                while not self.stop:
                    if not select.select([self.listener], [], [], .05)[0]: continue
                    peer, _ = self.listener.accept(); self.connections += 1
                    upstream = socket.create_connection(self.address, timeout=2)
                    self.sockets.extend([peer, upstream]); pending = [b'', b'']
                    while not self.stop:
                        ended = False
                        for origin in select.select([peer, upstream], [], [], .05)[0]:
                            data = origin.recv(65536)
                            if not data: ended = True; break
                            i = 0 if origin is peer else 1
                            stream = sent if i == 0 else received
                            assert stream.tell()+len(data) <= 64*1024*1024
                            stream.write(data); stream.flush(); pending[i] += data
                            assert len(pending[i]) <= 1048576
                            while b'\n' in pending[i]:
                                line, pending[i] = pending[i].split(b'\n', 1)
                                value = json.loads(line)
                                if i == 0:
                                    self.requests.append(value)
                                    assert len(self.requests) <= 1024
                                elif value.get('result', {}).get('time_anchor') or (value.get('result', {}).get('boot_id') and 'scope' in value['result']):
                                    self.hello = value['result']
                            (upstream if i == 0 else peer).sendall(data)
                        if ended: break
                    peer.close(); upstream.close()
        except OSError as error:
            if not self.stop: self.errors.append(str(error))
        finally:
            for stream in self.sockets: stream.close()


def bb(endpoint, mode):
    path = OUT/('bb-'+mode+'.json')
    with (OUT/('bb-'+mode+'.log')).open('wb') as log:
        reply = subprocess.run(['bb', str(ROOT/'clients/babashka-smoke/time1.clj'), endpoint.split(':')[1], mode, str(path)],
                               cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=20)
    assert reply.returncode == 0, (OUT/('bb-'+mode+'.log')).read_text(encoding='utf-8')
    value = common.read_json(path); assert value['passed']; return value


def capture(directory, revision, predicate=lambda _: True, after=0):
    value = common.wait(lambda: common.receipts(directory, lambda v: v['document_revision'] == revision
        and v['ui_counter'] > after and v['display']['scalars'] and v['display']['scalars'][0]['freshness'] == 'Fresh'
        and v['display']['plots'] and v['display']['plots'][0]['points'] >= 3 and predicate(v)), 'TIME-1 live window', 25)
    captures.append(str(value[0])); return value[1]


runtime = parent = relay = api = None
captures = []
result = dict(passed=False, profile=PROFILE, renderer=RENDERER, compatibility=COMPAT,
              source=subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
              binaries={str(p.relative_to(ROOT)): common.hashlib.sha256(p.read_bytes()).hexdigest() for p in (RUNTIME,PARENT,CHILD)},
              activation_qualified=False)
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=all','--','apps','crates','scripts','clients','Cargo.toml','Cargo.lock'], text=True).strip(), 'Commit executable inputs before qualification'
try:
    config = (ROOT/'examples/runtime.virtual.toml').read_text(encoding='utf-8')
    config = config.replace('port = 7420','port = 0').replace('initial_temperature = 20.0','initial_temperature = 80.0')
    config = config.replace('time_constant_ms = 8000','time_constant_ms = 120000')
    # Recorder-disabled run demonstrates the handshake's independent time source.
    config = config.replace('[recording]\nenabled = true', '[recording]\nenabled = false')
    config = config.replace('path = "history.sqlite"', 'path = "'+str(OUT/'history.sqlite').replace('\\','/')+'"')
    (OUT/'runtime.toml').write_text(config, encoding='utf-8')
    runtime = common.Process('runtime', [RUNTIME,'--serve','--config',OUT/'runtime.toml'])
    ready = common.wait(lambda: next((v for v in runtime.lines if v.get('state') == 'ready' and 'port' in v),None),'Runtime')
    relay = Relay(('127.0.0.1', int(ready['port'])))
    parent = common.Process('parent', [PARENT,'--connect',f'127.0.0.1:{relay.port}','--workspace',OUT/'workspace',
        '--workbench-listen','127.0.0.1:0','--experimental-eframe',RENDERER,'--experimental-eframe-child',CHILD,
        '--experimental-eframe-commit-limit','1073741824','--experimental-eframe-restart-limit','1','--experimental-eframe-capture'])
    endpoint = common.wait(lambda: next((v['workbench_endpoint'] for v in parent.lines if 'workbench_endpoint' in v),None),'Workbench API')
    api = common.API(endpoint)
    baseline = common.wait(lambda: (lambda s: s if s['runtime_client']['freshness'] == 'fresh' else None)(api.call('client_status',{})), 'Fresh')
    wire_before = len(relay.requests)
    frames = {}
    modes = ['default'] if COMPAT == 'old-parent' else ['default','utc','runtime_elapsed','local_calendar','experiment_elapsed']
    for mode in modes:
        applied = bb(endpoint, mode)
        wid = applied['workbench_id']; revision = int(applied['presentation_revision'])
        directory = CHILD.parent/'target/eframe-2-parent'/f'{parent.p.pid}-{wid}-1'
        child = common.wait(lambda: common.read_json(directory/'ready.json'), 'stock window')
        first = capture(directory, revision)
        second = capture(directory, revision, lambda v: v['display']['plots'][0]['last'] != first['display']['plots'][0]['last'], first['ui_counter'])
        plot = second['display']['plots'][0]
        expected = ('RuntimeElapsed' if COMPAT != 'current' or mode in ('runtime_elapsed','experiment_elapsed') else
                    'Utc' if mode == 'utc' else 'LocalCalendar')
        assert plot['time_mode'] == expected, plot
        if expected == 'Utc':
            anchor = relay.hello['time_anchor']; x, _ = plot['last']
            utc = datetime.datetime.fromtimestamp(int(anchor['utc_unix_us'])/1e6+x-int(anchor['monotonic_after_ns'])/1e9, datetime.timezone.utc)
            assert utc.strftime('%H:%M:%S') == plot['time_label'], (utc, plot)
            assert anchor['boot_id'] == ready['boot_id'] and anchor['utc_accuracy_ns'] is None
        if mode == 'experiment_elapsed': assert 'Experiment start unavailable' in plot['time_note']
        assert second['display']['scalars'][0]['unit'] == plot['unit'] == '°C'
        assert second['display']['scalars'][0]['value'] != first['display']['scalars'][0]['value']
        frames[mode] = second
    # Keep configuration in the Parent through a GUI-only crash/restart.
    subprocess.run(['taskkill','/PID',str(child['pid']),'/F'], check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    directory2 = CHILD.parent/'target/eframe-2-parent'/f'{parent.p.pid}-{wid}-2'
    restarted = common.wait(lambda: common.read_json(directory2/'ready.json'), 'same renderer restart')
    restored = capture(directory2, revision)
    assert restored['display']['plots'][0]['time_mode'] == frames[modes[-1]]['display']['plots'][0]['time_mode']
    old_exit = common.wait(lambda: next((v for v in parent.lines if v.get('incarnation') == 1 and v.get('phase') == 'ExitedContained'), None),'old IPC retirement')
    assert old_exit['ipc_retired'] and old_exit['ipc_handles'] == old_exit['ipc_pending_io'] == 0 and old_exit['objects'] == [0,0,0]
    after = api.call('client_status',{})
    assert after['runtime_client']['hello'] == baseline['runtime_client']['hello']
    assert relay.connections == 1 and len(relay.requests) == wire_before
    assert not any('request_id' in r or r['op'] in ('operation_status','exact_retry') for r in relay.requests)
    common.original_window_close(restarted)
    assert parent.p.wait(timeout=15) == 0 and runtime.p.poll() is None
    final = common.wait(lambda: next((v for v in parent.lines if v.get('incarnation') == 2 and v.get('phase') == 'ExitedHealthy'), None),'normal IPC retirement')
    assert final['ipc_retired'] and final['ipc_handles'] == final['ipc_pending_io'] == 0 and final['objects'] == [0,0,0]
    result.update(passed=True, runtime_connections=relay.connections, display_caused_requests=len(relay.requests)-wire_before,
                  runtime_hello=relay.hello, requests=relay.requests, frames=frames, restored=restored,
                  captures=captures, old_retirement=old_exit, final_retirement=final,
                  framebuffer_verification=[common.verify_png(Path(p).with_suffix('.png')) for p in captures],
                  parent_status=parent.lines, child_directories=[str(directory),str(directory2)])
finally:
    if api: common.write(OUT/'workbench-api.json',api.transcript); api.close()
    if parent and parent.p.poll() is None: parent.p.terminate(); parent.p.wait(timeout=10)
    if runtime and runtime.p.poll() is None: runtime.p.terminate(); runtime.p.wait(timeout=10)
    if relay: relay.close(); result['relay_errors'] = relay.errors
    if parent: parent.finish()
    if runtime: runtime.finish()
    common.write(OUT/'result.json',result)
print(json.dumps(dict(passed=result['passed'], output=str(OUT))))
