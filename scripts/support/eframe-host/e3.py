"""Finite Windows user experiment: external Babashka -> existing Parent -> stock UI.

Requires built dev/release binaries and bb on PATH. Never connects a second
Runtime client; the transparent relay only records the Parent's original bytes.
All native input is scoped to the original child PID/HWND. Keep each output,
including the virtual Recorder DB and WAL/SHM. No laboratory equipment.
"""
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import subprocess
import sys
import time
import e2 as common

ROOT = common.ROOT
PROFILE, RENDERER, RUN = sys.argv[1:4]
assert PROFILE in ('dev', 'release') and RENDERER in ('wgpu', 'glow')
assert RUN.replace('-', '').isalnum()
OUT = ROOT / 'target/eframe-3' / RUN
OUT.mkdir(parents=True, exist_ok=False)
common.OUT = OUT
BIN = 'debug' if PROFILE == 'dev' else 'release'
RUNTIME = ROOT / 'target' / BIN / 'lab-runtime.exe'
PARENT = ROOT / 'target' / BIN / 'lab-workbench.exe'
CHILD = ROOT / 'scripts/support/eframe-host/target' / BIN / 'lab-eframe-host.exe'
user = common.user
user.SetProcessDpiAwarenessContext.argtypes = [c.c_void_p]
assert user.SetProcessDpiAwarenessContext(c.c_void_p(-4))
for name, arguments in [('ShowWindow', [w.HWND, c.c_int]), ('IsIconic', [w.HWND]),
                        ('SetForegroundWindow', [w.HWND]), ('GetDpiForWindow', [w.HWND]),
                        ('ClientToScreen', [w.HWND, c.POINTER(w.POINT)]),
                        ('SetCursorPos', [c.c_int, c.c_int])]:
    getattr(user, name).argtypes = arguments
user.GetForegroundWindow.restype = w.HWND
user.AttachThreadInput.argtypes = [w.DWORD, w.DWORD, w.BOOL]
user.SetFocus.argtypes = [w.HWND]
kernel = c.WinDLL('kernel32', use_last_error=True)
kernel.GetCurrentThreadId.restype = w.DWORD

class Mouse(c.Structure):
    _fields_ = [('x', w.LONG), ('y', w.LONG), ('data', w.DWORD), ('flags', w.DWORD), ('time', w.DWORD), ('extra', c.c_size_t)]
class Payload(c.Union):
    _fields_ = [('mouse', Mouse), ('padding', c.c_byte * 32)]
class Input(c.Structure):
    _fields_ = [('kind', w.DWORD), ('payload', Payload)]
assert c.sizeof(Input) == 40
user.SendInput.argtypes = [w.UINT, c.POINTER(Input), c.c_int]
actions = []

def owner(ready):
    pid = w.DWORD()
    user.GetWindowThreadProcessId(ready['hwnd'], c.byref(pid))
    assert pid.value == ready['pid'], 'original HWND/PID'

def focus(ready):
    owner(ready)
    if user.GetForegroundWindow() == ready['hwnd']: return
    user.GetQueueStatus(0x4ff)
    current = kernel.GetCurrentThreadId()
    threads = {user.GetWindowThreadProcessId(ready['hwnd'], None)}
    foreground = user.GetForegroundWindow()
    if foreground: threads.add(user.GetWindowThreadProcessId(foreground, None))
    attached = []
    try:
        for thread in threads - {0, current}:
            assert user.AttachThreadInput(current, thread, True)
            attached.append(thread)
        user.SetForegroundWindow(ready['hwnd']); user.SetFocus(ready['hwnd'])
    finally:
        for thread in reversed(attached): assert user.AttachThreadInput(current, thread, False)
    assert user.GetForegroundWindow() == ready['hwnd']

def move(ready, x, y):
    owner(ready)
    scale = user.GetDpiForWindow(ready['hwnd']) / 96.
    point = w.POINT(round(x*scale), round(y*scale))
    assert user.ClientToScreen(ready['hwnd'], c.byref(point))
    assert user.SetCursorPos(point.x, point.y)

def mouse(ready, flags, data=0):
    focus(ready)
    event = Input(); event.payload.mouse = Mouse(0, 0, data & 0xffffffff, flags, 0, 0)
    assert user.SendInput(1, c.byref(event), c.sizeof(Input)) == 1

def click(ready, rect):
    move(ready, (rect[0]+rect[2])/2, (rect[1]+rect[3])/2)
    time.sleep(.06)
    mouse(ready, 2); time.sleep(.06); mouse(ready, 4)
    time.sleep(.15)

def bb(endpoint, stage, name):
    path = OUT / (name+'.json')
    command = ['bb', str(ROOT/'clients/babashka-smoke/eframe3.clj'), endpoint.split(':')[1], stage, str(path)]
    with (OUT/(name+'.log')).open('wb') as stream:
        reply = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, timeout=20)
    assert reply.returncode == 0, (OUT/(name+'.log')).read_text(encoding='utf-8')
    value = common.read_json(path); assert value['passed']
    return value

def capture(directory, predicate=lambda _:True, after=0):
    value = common.wait(lambda: common.receipts(directory, lambda v: v['ui_counter']>after and predicate(v)), 'stock screenshot', 25)
    captures.append(str(value[0])); return value[1]

def navigation(ready, frame, label, directory, predicate):
    response = next(w for w in frame['display']['widgets'] if w['label'] == label)
    assert response['enabled']
    focus(ready); click(ready, response['rect'])
    actions.append(dict(native_click=label, rect=response['rect']))
    return capture(directory, predicate, frame['ui_counter'])

runtime = parent = relay = api = None
captures = []
result = dict(passed=False, profile=PROFILE, renderer=RENDERER,
              source=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),
              dirty=subprocess.check_output(['git','status','--porcelain'],text=True),
              binaries={p.name:common.hashlib.sha256(p.read_bytes()).hexdigest() for p in (RUNTIME,PARENT,CHILD)},
              activation_qualified=False)
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=all','--','apps','crates','scripts','clients','Cargo.toml','Cargo.lock'],text=True).strip(), 'Commit executable inputs before qualification'
try:
    config = (ROOT/'examples/runtime.virtual.toml').read_text(encoding='utf-8')
    config = config.replace('port = 7420','port = 0').replace('initial_temperature = 20.0','initial_temperature = 80.0')
    config = config.replace('time_constant_ms = 8000','time_constant_ms = 120000')
    config = config.replace('path = "history.sqlite"', 'path = "'+str(OUT/'history.sqlite').replace('\\','/')+'"')
    (OUT/'runtime.toml').write_text(config,encoding='utf-8')
    runtime = common.Process('runtime',[RUNTIME,'--serve','--config',OUT/'runtime.toml'])
    ready = common.wait(lambda:next((v for v in runtime.lines if v.get('state')=='ready' and 'port'in v),None),'virtual furnace Runtime')
    relay = common.Relay(('127.0.0.1',int(ready['port'])))
    parent = common.Process('parent',[PARENT,'--connect',f'127.0.0.1:{relay.port}','--workspace',OUT/'workspace',
        '--workbench-listen','127.0.0.1:0','--experimental-eframe',RENDERER,'--experimental-eframe-child',CHILD,
        '--experimental-eframe-commit-limit','1073741824','--experimental-eframe-restart-limit','1','--experimental-eframe-capture'])
    endpoint = common.wait(lambda:next((v['workbench_endpoint'] for v in parent.lines if 'workbench_endpoint'in v),None),'Workbench API')
    api = common.API(endpoint)
    baseline = common.wait(lambda:(lambda s:s if s['runtime_client']['freshness']=='fresh' else None)(api.call('client_status',{})),'Fresh rebuild')
    initial = bb(endpoint,'initial','bb-initial')
    wid = initial['workbench_id']
    directory = CHILD.parent/'target/eframe-2-parent'/f'{parent.p.pid}-{wid}-1'
    child = common.wait(lambda:common.read_json(directory/'ready.json'),'original native window')
    def live(v):
        d=v['display']
        return d['scalars'] and d['scalars'][0]['freshness']=='Fresh' and d['plots'] and d['plots'][0]['points']>=3
    first = capture(directory,live)
    second = capture(directory,lambda v:live(v) and v['display']['scalars'][0]['value']!=first['display']['scalars'][0]['value'],first['ui_counter'])
    assert second['display']['plots'][0]['last']!=first['display']['plots'][0]['last']
    assert second['display']['scalars'][0]['unit']=='°C'
    wire = len(relay.requests)
    focus(child)
    # Clear only fixture input in our original focused HWND (a failed prior
    # driver can otherwise leave a native button held on this test desktop).
    mouse(child,4); mouse(child,16)
    bounds = second['display']['views'][0]['bounds']
    box = second['display']['views'][0]['rect']; x=(box[0]+box[2])/2; y=(box[1]+box[3])/2
    move(child,x,y); mouse(child,2); time.sleep(.08); move(child,x+70,y+20); time.sleep(.08); mouse(child,4)
    panned = capture(directory,lambda v:live(v) and v['display']['views'][0]['auto']==[False,False],second['ui_counter'])
    assert panned['display']['views'][0]['bounds']!=bounds
    move(child,x-80,y-40); mouse(child,8); time.sleep(.15)
    move(child,x+80,y+40); time.sleep(.15); mouse(child,16)
    zoomed = capture(directory,lambda v:live(v) and v['display']['views'][0]['bounds']!=panned['display']['views'][0]['bounds'],panned['ui_counter'])
    span=lambda v:v['display']['views'][0]['bounds'][1][0]-v['display']['views'][0]['bounds'][0][0]
    assert span(zoomed)<span(panned)
    # Stock legend is in the upper-right of the measured stock plot frame.
    click(child,[box[2]-100,box[1]+5,box[2]-5,box[1]+20])
    hidden = capture(directory,lambda v:live(v) and v['display']['views'][0]['hidden']==1,zoomed['ui_counter'])
    actions.append(dict(pan=True,zoom=True,zoom_input="stock secondary-button box",legend=True,bounds_before=bounds,bounds_pan=panned['display']['views'][0]['bounds'],bounds_zoom=zoomed['display']['views'][0]['bounds']))
    only_scalar = navigation(child,hidden,'Next panel',directory,lambda v:v['display']['panels']==['Furnace temperature'])
    only_plot = navigation(child,only_scalar,'Next panel',directory,lambda v:v['display']['panels']==['Temperature history'])
    all_panels = navigation(child,only_plot,'All panels',directory,lambda v:len(v['display']['panels'])==2)
    diagnostics = navigation(child,all_panels,'Next tab',directory,lambda v:'Absent source probe'in v['display']['panels'])
    assert diagnostics['display']['scalars'][0]['freshness'].startswith('Unavailable')
    controls = navigation(child,diagnostics,'Next tab',directory,lambda v:'Six groups, all actions disabled'in v['display']['panels'])
    inert = [v for v in controls['display']['widgets'] if v['label'] in ('New value','Action')]
    assert len(inert)==12 and all(not v['enabled'] for v in inert)
    # Exercise a disabled native action and confirm API/revision/Runtime identity below.
    focus(child); click(child,next(v['rect'] for v in inert if v['label']=='Action'))
    other = navigation(child,controls,'Next window',directory,lambda v:v['display']['heading']=='Second configured presentation window')
    back = navigation(child,other,'Previous window',directory,live)
    verified = bb(endpoint,'verify','bb-after-navigation')
    assert verified['document']==initial['document'] and verified['presentation_revision']==initial['presentation_revision']
    assert relay.connections==1 and len(relay.requests)==wire
    # Remain minimized far beyond the Supervisor's eight-second observation deadline.
    owner(child); user.ShowWindow(child['hwnd'],6)
    common.wait(lambda:user.IsIconic(child['hwnd']),'actual minimize')
    time.sleep(2)
    start = common.read_json(directory/'lifecycle.json'); assert start['minimized']
    minimized_at=time.monotonic()
    updated = bb(endpoint,'updated','bb-while-minimized')
    until=time.monotonic()+120
    while time.monotonic()<until:
        assert parent.p.poll() is None and runtime.p.poll() is None
        owner(child); assert user.IsIconic(child['hwnd'])
        time.sleep(.2)
    end = common.read_json(directory/'lifecycle.json')
    assert end['owner_counter']>start['owner_counter'] and end['ui_counter']==start['ui_counter']
    assert end['observation_snapshot']>start['observation_snapshot']
    assert end['document_revision']==int(updated['presentation_revision'])
    assert not any(v.get('incarnation',0)>1 for v in parent.lines)
    minimized_seconds=time.monotonic()-minimized_at
    user.ShowWindow(child['hwnd'],9); focus(child)
    restored = capture(directory,lambda v:live(v) and v['display']['heading']=='EFRAME-3 API updated furnace',back['ui_counter'])
    actions.append(dict(minimized_seconds=minimized_seconds,logic_before=start,logic_after=end,same_child_pid=child['pid'],same_incarnation=1))
    assert relay.connections==1 and len(relay.requests)==wire
    before_fault = bb(endpoint,'verify','bb-before-crash')
    subprocess.run(['taskkill','/PID',str(child['pid']),'/F'],check=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
    directory2=CHILD.parent/'target/eframe-2-parent'/f'{parent.p.pid}-{wid}-2'
    child2=common.wait(lambda:common.read_json(directory2/'ready.json'),'controlled restart')
    recovered=capture(directory2,lambda v:live(v) and v['display']['heading']=='EFRAME-3 API updated furnace')
    terminal=common.wait(lambda:next((v for v in parent.lines if v.get('incarnation')==1 and v.get('phase')=='ExitedContained'),None),'original retirement')
    assert terminal['objects']==[0,0,0] and terminal['ipc_retired'] and terminal['ipc_handles']==terminal['ipc_pending_io']==0
    after_fault=bb(endpoint,'verify','bb-after-crash')
    assert before_fault['runtime_attachment']==after_fault['runtime_attachment']==initial['runtime_attachment']
    assert before_fault['document']==after_fault['document']==updated['document']
    assert relay.connections==1 and len(relay.requests)==wire and runtime.p.poll() is None
    # A separate, explicit virtual Runtime-loss injection verifies Stale. This is
    # after GUI continuity assertions; it is not attributed to the GUI crash.
    runtime.p.terminate(); runtime.p.wait(timeout=10)
    stale=capture(directory2,lambda v:v['display']['scalars'] and v['display']['scalars'][0]['freshness']=='Stale',recovered['ui_counter'])
    common.original_window_close(child2)
    assert parent.p.wait(timeout=15)==0
    final=common.wait(lambda:next((v for v in parent.lines if v.get('incarnation')==2 and v.get('phase')=='ExitedHealthy'),None),'normal close retirement')
    assert final['objects']==[0,0,0] and final['ipc_retired'] and final['ipc_handles']==final['ipc_pending_io']==0
    assert common.read_json(directory2/'on-exit.json')['closed_write_completed']
    assert not [v for v in relay.requests if 'seq'in v or v.get('op')in ('operation_status','exact_retry')]
    result.update(passed=True, workbench_id=wid, runtime_connections_before_independent_runtime_loss=1,
        runtime_attachment=initial['runtime_attachment'],requests=relay.requests,
        minimized_before=start,minimized_after=end,crash_retirement=terminal,normal_retirement=final,
        first_window=child,restarted_window=child2,child_directories=[str(directory),str(directory2)])
finally:
    if 'child' in globals() and parent and parent.p.poll() is None:
        try:
            owner(child); focus(child); mouse(child,4); mouse(child,16)
        except AssertionError: pass
    result['captures']=captures; result['actions']=actions
    if api: common.write(OUT/'observer-api.json',api.transcript); api.close()
    if parent and parent.p.poll() is None: parent.p.terminate(); parent.p.wait(timeout=10)
    if runtime and runtime.p.poll() is None: runtime.p.terminate(); runtime.p.wait(timeout=10)
    if relay: relay.close(); result['relay_errors']=relay.errors
    if parent: parent.finish()
    if runtime: runtime.finish()
    common.write(OUT/'result.json',result)
print(json.dumps(dict(passed=result['passed'],output=str(OUT))))
