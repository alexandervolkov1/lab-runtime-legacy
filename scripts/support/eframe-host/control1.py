"""Finite CONTROL-1: external Babashka document -> real window -> one Parent mutation.

No Runtime client in this harness. The byte relay preserves order and creates no
requests. Fault mode holds terminal delivery for <= 4 seconds, then crashes only
the original GUI; Runtime/Parent remain alive. Keep every failed run and DB/WAL.
"""
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import select
import socket
import subprocess
import sys
import threading
import time
import e2 as common

ROOT = common.ROOT
PROFILE, RENDERER, RUN, MODE = sys.argv[1:5]
assert PROFILE in ('dev', 'release') and RENDERER in ('wgpu', 'glow')
assert MODE in ('fault', 'smoke', 'observe') and RUN.replace('-', '').isalnum()
OUT = ROOT/'target/control-1'/RUN
OUT.mkdir(parents=True, exist_ok=False)
common.OUT = OUT
BIN = 'debug' if PROFILE == 'dev' else 'release'
RUNTIME = ROOT/'target'/BIN/'lab-runtime.exe'
PARENT = ROOT/'target'/BIN/'lab-workbench.exe'
CHILD = ROOT/'scripts/support/eframe-host/target'/BIN/'lab-eframe-host.exe'
user = common.user
user.SetProcessDpiAwarenessContext.argtypes = [c.c_void_p]
assert user.SetProcessDpiAwarenessContext(c.c_void_p(-4))
for name, args in [('SetForegroundWindow', [w.HWND]), ('SetFocus', [w.HWND]),
                   ('GetDpiForWindow', [w.HWND]), ('ClientToScreen', [w.HWND, c.POINTER(w.POINT)]),
                   ('SetCursorPos', [c.c_int, c.c_int]), ('AttachThreadInput', [w.DWORD, w.DWORD, w.BOOL]),
                   ('SetWindowPos', [w.HWND,w.HWND,c.c_int,c.c_int,c.c_int,c.c_int,w.UINT]),
                   ('GetCursorPos', [c.POINTER(w.POINT)]), ('GetWindowRect', [w.HWND,c.POINTER(w.RECT)]),
                   ('IsIconic', [w.HWND]), ('ShowWindow', [w.HWND,c.c_int])]:
    getattr(user, name).argtypes = args
user.GetForegroundWindow.restype = w.HWND
kernel = c.WinDLL('kernel32', use_last_error=True)
kernel.GetCurrentThreadId.restype = w.DWORD

class Mouse(c.Structure):
    _fields_ = [('x',w.LONG),('y',w.LONG),('data',w.DWORD),('flags',w.DWORD),('time',w.DWORD),('extra',c.c_size_t)]
class Keyboard(c.Structure):
    _fields_ = [('virtual',w.WORD),('scan',w.WORD),('flags',w.DWORD),('time',w.DWORD),('extra',c.c_size_t)]
class Payload(c.Union):
    _fields_ = [('mouse',Mouse),('keyboard',Keyboard)]
class Input(c.Structure):
    _fields_ = [('kind',w.DWORD),('payload',Payload)]
assert c.sizeof(Input) == 40
user.SendInput.argtypes = [w.UINT,c.POINTER(Input),c.c_int]
user.SendInput.restype = w.UINT

def owner(ready):
    pid = w.DWORD()
    user.GetWindowThreadProcessId(ready['hwnd'],c.byref(pid))
    assert pid.value == ready['pid'], 'original HWND/PID'

def place(ready):
    owner(ready)
    # Deterministic test placement prevents OS cascade/default positioning from
    # putting a requested native input point outside the interactive desktop.
    assert user.SetWindowPos(ready['hwnd'],None,20,20,0,0,0x15)

def focus(ready):
    owner(ready)
    if user.IsIconic(ready['hwnd']):
        user.ShowWindow(ready['hwnd'],9)
        common.wait(lambda:not user.IsIconic(ready['hwnd']),'restore original test window',3)
    if user.GetForegroundWindow() == ready['hwnd']: return
    user.GetQueueStatus(0x4ff)
    current = kernel.GetCurrentThreadId()
    threads = {user.GetWindowThreadProcessId(ready['hwnd'],None)}
    foreground = user.GetForegroundWindow()
    if foreground: threads.add(user.GetWindowThreadProcessId(foreground,None))
    attached = []
    try:
        for thread in threads - {0,current}:
            assert user.AttachThreadInput(current,thread,True)
            attached.append(thread)
        user.SetForegroundWindow(ready['hwnd']); user.SetFocus(ready['hwnd'])
    finally:
        for thread in reversed(attached): assert user.AttachThreadInput(current,thread,False)
    assert user.GetForegroundWindow() == ready['hwnd']

def inject(ready, event):
    focus(ready); owner(ready); assert user.GetForegroundWindow() == ready['hwnd']
    assert user.SendInput(1,c.byref(event),c.sizeof(Input)) == 1

def key(ready, virtual, up=False):
    event=Input(); event.kind=1; event.payload.keyboard=Keyboard(virtual,0,2 if up else 0,0,0)
    inject(ready,event)

def click(ready, rect):
    focus(ready); scale=user.GetDpiForWindow(ready['hwnd'])/96.
    point=w.POINT(round((rect[0]+rect[2])/2*scale),round((rect[1]+rect[3])/2*scale))
    assert user.ClientToScreen(ready['hwnd'],c.byref(point)); assert user.SetCursorPos(point.x,point.y)
    actual=w.POINT();assert user.GetCursorPos(c.byref(actual))
    assert (actual.x,actual.y)==(point.x,point.y),'native point was clipped outside the desktop'
    time.sleep(.06)
    for flags in (2,4):
        event=Input();event.payload.mouse=Mouse(0,0,0,flags,0,0);inject(ready,event);time.sleep(.06)
    time.sleep(.12)

def replace_text(ready, rect, text):
    click(ready,rect)
    key(ready,0x11);key(ready,0x41);key(ready,0x41,True);key(ready,0x11,True)
    for char in text:
        for flags in (4,6):
            event=Input();event.kind=1;event.payload.keyboard=Keyboard(0,ord(char),flags,0,0);inject(ready,event)
    time.sleep(.1)

class Relay(common.Relay):
    def __init__(self,address):
        self.held=threading.Event();self.release=threading.Event();self.responses=[];self.delay_once=MODE=='fault'
        super().__init__(address)
    def run(self):
        try:
            with (OUT/'runtime-client-wire.ndjson').open('ab') as sent, (OUT/'runtime-server-wire.ndjson').open('ab') as received:
                while not self.stop:
                    if not select.select([self.listener],[],[],.05)[0]:continue
                    peer,_=self.listener.accept();self.connections+=1
                    upstream=socket.create_connection(self.address,timeout=2);self.sockets.extend([peer,upstream]);pending=[b'',b'']
                    while not self.stop:
                        ended=False
                        for origin in select.select([peer,upstream],[],[],.05)[0]:
                            data=origin.recv(65536)
                            if not data:ended=True;break
                            i=0 if origin is peer else 1;stream=sent if i==0 else received
                            assert stream.tell()+len(data)<16*1024*1024
                            stream.write(data);stream.flush();pending[i]+=data;assert len(pending[i])<=1048576
                            while b'\n' in pending[i]:
                                line,pending[i]=pending[i].split(b'\n',1);value=json.loads(line)
                                records=self.requests if i==0 else self.responses;records.append(value);assert len(records)<=8192
                                if i==1 and self.delay_once and value.get('type')=='operation' and value.get('state')=='completed':
                                    self.delay_once=False;self.held.set();start=time.monotonic()
                                    while not self.release.wait(.02) and not self.stop:
                                        assert time.monotonic()-start<4,'finite terminal-delivery hold exceeded'
                                (upstream if i==0 else peer).sendall(line+b'\n')
                        if ended:break
                    peer.close();upstream.close()
        except (OSError,AssertionError) as error:
            if not self.stop:self.errors.append(str(error))
        finally:
            for stream in self.sockets:stream.close()

def bb(endpoint, stage, name):
    path=OUT/(name+'.json')
    with (OUT/(name+'.log')).open('wb') as log:
        reply=subprocess.run(['bb',str(ROOT/'clients/babashka-smoke/control1.clj'),endpoint.split(':')[1],stage,str(path)],
                             cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,timeout=20,creationflags=subprocess.CREATE_NO_WINDOW)
    assert reply.returncode==0,(OUT/(name+'.log')).read_text(encoding='utf-8')
    value=common.read_json(path);assert value['passed'];return value

captures=[];actions=[]
def capture(directory,predicate=lambda _:True,after=0):
    focus(child)
    value=common.wait(lambda:common.receipts(directory,lambda v:v['ui_counter']>after and predicate(v)),'CONTROL-1 stock screenshot',20)
    captures.append(str(value[0]));return value[1]
def widget(frame,label):return next(w for w in frame['display']['widgets'] if w['label']==label)
def press(ready,frame,label):
    button=widget(frame,label);assert button['enabled'],(label,button)
    click(ready,button['rect']);actions.append({'click':label,'rect':button['rect']})
def state(frame):return frame['display'].get('control_state') or {}
def draft(frame):return next(v for v in frame['display']['controls'] if v['id']=='retune')
def live(frame):return bool(frame['display']['scalars']) and frame['display']['scalars'][0]['freshness']=='Fresh'
def mutations(relay):return [r for r in relay.requests if 'request_id' in r]

runtime=parent=relay=api=None;child=None
result=dict(passed=False,profile=PROFILE,renderer=RENDERER,mode=MODE,
    source=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),
    binaries={str(p.relative_to(ROOT)):common.hashlib.sha256(p.read_bytes()).hexdigest() for p in (RUNTIME,PARENT,CHILD)},activation_qualified=False)
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=all','--','apps','crates','scripts','clients','Cargo.toml','Cargo.lock'],text=True).strip(),'Commit executable inputs before qualification'
try:
    config=(ROOT/'examples/runtime.virtual.toml').read_text(encoding='utf-8')
    config=config.replace('port = 7420','port = 0').replace('initial_temperature = 20.0','initial_temperature = 80.0')
    config=config.replace('path = "history.sqlite"','path = "'+str(OUT/'history.sqlite').replace('\\','/')+'"')
    (OUT/'runtime.toml').write_text(config,encoding='utf-8')
    runtime=common.Process('runtime',[RUNTIME,'--serve','--config',OUT/'runtime.toml'])
    ready=common.wait(lambda:next((v for v in runtime.lines if v.get('state')=='ready' and 'port'in v),None),'virtual Runtime')
    relay=Relay(('127.0.0.1',int(ready['port'])))
    arguments=[PARENT,'--connect',f'127.0.0.1:{relay.port}','--workspace',OUT/'workspace','--workbench-listen','127.0.0.1:0',
        '--experimental-eframe',RENDERER,'--experimental-eframe-child',CHILD,'--experimental-eframe-commit-limit','1073741824',
        '--experimental-eframe-restart-limit','1','--experimental-eframe-capture','--experimental-eframe-controls']
    if MODE=='observe':arguments+=['--observe']
    parent=common.Process('parent',arguments)
    endpoint=common.wait(lambda:next((v['workbench_endpoint'] for v in parent.lines if 'workbench_endpoint'in v),None),'Workbench API')
    api=common.API(endpoint)
    baseline=common.wait(lambda:(lambda s:s if s['runtime_client']['freshness']=='fresh' else None)(api.call('client_status',{})),'Fresh rebuild')
    configured=bb(endpoint,'configure','bb-configure');wid=configured['workbench_id']
    directory=CHILD.parent/'target/eframe-2-parent'/f'{parent.p.pid}-{wid}-1'
    child=common.wait(lambda:common.read_json(directory/'ready.json'),'native stock window')
    place(child)
    # As in the existing native fixture, clear only input addressed to this
    # original foreground window; an interrupted driver can leave a button held.
    focus(child)
    for flags in (4,16):
        event=Input();event.payload.mouse=Mouse(0,0,0,flags,0,0);inject(child,event)
    frame=capture(directory,lambda v:live(v) and v['display']['controls'] and state(v).get('revision')==int(configured['presentation_revision']))
    initial_wire=len(relay.requests);assert not mutations(relay)
    replace_text(child,widget(frame,'retune: Target')['rect'],'NaN')
    replace_text(child,widget(frame,'retune: Rate')['rect'],'2')
    invalid=capture(directory,lambda v:draft(v)['target']=='NaN' and draft(v)['rate']=='2',frame['ui_counter'])
    assert not draft(invalid)['valid'] and not widget(invalid,'retune: Apply retune')['enabled']
    assert len(relay.requests)==initial_wire,'editing sent a Runtime request'
    replaced=bb(endpoint,'replace','bb-replace')
    frame=capture(directory,lambda v:v['document_revision']==int(replaced['presentation_revision']) and draft(v)['target']=='',invalid['ui_counter'])
    replace_text(child,widget(frame,'retune: Target')['rect'],'65')
    replace_text(child,widget(frame,'retune: Rate')['rect'],'3')
    edited=capture(directory,lambda v:draft(v)['target']=='65' and draft(v)['rate']=='3',frame['ui_counter'])
    assert len(relay.requests)==initial_wire and not mutations(relay)
    if MODE=='observe':
        assert not state(edited)['enabled'] and not widget(edited,'retune: Apply retune')['enabled']
        click(child,widget(edited,'retune: Apply retune')['rect']);time.sleep(.2)
        assert not mutations(relay)
        result['observation_blocked']=True
    else:
        press(child,edited,'retune: Apply retune')
        proposal=capture(directory,lambda v:state(v).get('stage')=='confirmation',edited['ui_counter'])
        assert not mutations(relay) and len(relay.requests)==initial_wire,'proposal sent a Runtime request'
        press(child,proposal,'Confirm retune')
        if MODE=='fault':
            common.wait(relay.held.is_set,'terminal response held',5)
            accepted=capture(directory,lambda v:state(v).get('stage')=='accepted',proposal['ui_counter'])
            assert state(accepted)['result'] is None
            original=child
            subprocess.run(['taskkill','/PID',str(original['pid']),'/F'],check=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=5,creationflags=subprocess.CREATE_NO_WINDOW)
            relay.release.set()
            assert runtime.p.poll() is None and parent.p.poll() is None
            directory=CHILD.parent/'target/eframe-2-parent'/f'{parent.p.pid}-{wid}-2'
            child=common.wait(lambda:common.read_json(directory/'ready.json'),'controlled GUI restart')
            place(child)
            retired=common.wait(lambda:next((v for v in parent.lines if v.get('incarnation')==1 and v.get('phase')=='ExitedContained'),None),'old process/IPC retirement')
            assert retired['objects']==[0,0,0] and retired['ipc_retired'] and retired['ipc_handles']==retired['ipc_pending_io']==0
            result.update(crash_retirement=retired,original_window=original)
        completed=capture(directory,lambda v:live(v) and state(v).get('stage')=='completed')
        response=state(completed)['result'];assert response['reference']=='1' and response['target']==65 and response['rate']==3
        assert state(completed)['accepted_observed'] and len(mutations(relay))==1
        actual=mutations(relay)[0];assert actual['op']=='reference_retune' and actual['args']['target']==65 and actual['args']['rate']==3
        assert len(relay.requests)==initial_wire+1,'display/restart caused extra Runtime requests'
        progressed=capture(directory,lambda v:live(v) and v['display']['scalars'][0]['value']!=completed['display']['scalars'][0]['value'],completed['ui_counter'])
        press(child,progressed,'Acknowledge result')
        idle=capture(directory,lambda v:state(v).get('stage')=='idle',progressed['ui_counter'])
        assert len(mutations(relay))==1 and len(relay.requests)==initial_wire+1
        result.update(operation=actual,operation_result=response,accepted_observed=True,
            observed_before=completed['display']['scalars'][0],observed_after=progressed['display']['scalars'][0])
    final_api=bb(endpoint,'verify','bb-verify')
    assert final_api['runtime_attachment']['boot_id']==configured['runtime_attachment']['boot_id']
    assert final_api['runtime_attachment']['scope']==configured['runtime_attachment']['scope']
    assert relay.connections==1 and runtime.p.poll() is None
    common.original_window_close(child)
    assert parent.p.wait(timeout=15)==0
    final=common.wait(lambda:next((v for v in parent.lines if v.get('incarnation')==child['incarnation'] and v.get('phase')=='ExitedHealthy'),None),'normal retirement')
    assert final['objects']==[0,0,0] and final['ipc_retired'] and final['ipc_handles']==final['ipc_pending_io']==0
    assert common.read_json(directory/'on-exit.json')['closed_write_completed']
    assert runtime.p.poll() is None,'GUI close terminated Runtime'
    assert not [r for r in relay.requests if r.get('op') in ('operation_status','exact_retry')]
    assert not relay.errors
    result.update(passed=True,runtime_connections=relay.connections,runtime_attachment=final_api['runtime_attachment'],
        requests=relay.requests,normal_retirement=final,final_window=child,
        child_directories=[str(CHILD.parent/'target/eframe-2-parent'/f'{parent.p.pid}-{wid}-{n}') for n in range(1,child['incarnation']+1)])
finally:
    if relay:relay.release.set()
    if child and parent and parent.p.poll() is None:
        try:
            key(child,0x11,True)
            for flags in (4,16):
                event=Input();event.payload.mouse=Mouse(0,0,0,flags,0,0);inject(child,event)
        except AssertionError:pass
    if api:common.write(OUT/'observer-api.json',api.transcript);api.close()
    if relay:relay.close();result['relay_errors']=relay.errors
    if parent and parent.p.poll() is None:parent.p.terminate();parent.p.wait(timeout=10)
    if runtime and runtime.p.poll() is None:runtime.p.terminate();runtime.p.wait(timeout=10)
    if parent:parent.finish()
    if runtime:runtime.finish()
    result.update(captures=captures,actions=actions)
    common.write(OUT/'result.json',result)
print(json.dumps({'passed':result['passed'],'output':str(OUT)}))
