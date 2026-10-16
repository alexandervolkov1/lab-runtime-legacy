"""Finite Windows E2E: real virtual Runtime -> one Workbench Parent -> stock child.

The relay observes original NDJSON bytes. It never creates a Runtime scope or
request. OS operations target only this run's original process/window handles.
No laboratory hardware, input commands, graphics API or third-party Python package.
"""
import ctypes as c
from ctypes import wintypes as w
import datetime
import hashlib
import json
from pathlib import Path
import select
import socket
import subprocess
import sys
import threading
import struct
import zlib
import time

ROOT=Path(__file__).resolve().parents[3]
if __name__ == "__main__":
    PROFILE,RENDERER,RUN=sys.argv[1:4]
    SCENARIO=sys.argv[4] if len(sys.argv)>4 else 'fault'
    assert SCENARIO in ('fault','normal')
    assert PROFILE in ('dev','release') and RENDERER in ('wgpu','glow')
    assert RUN.replace('-','').isalnum()
    OUT=ROOT/'target/eframe-2'/RUN
    OUT.mkdir(parents=True,exist_ok=False)
    BIN='debug' if PROFILE=='dev' else 'release'
    RUNTIME=ROOT/'target'/BIN/'lab-runtime.exe'
    PARENT=ROOT/'target'/BIN/'lab-workbench.exe'
    CHILD=ROOT/'scripts/support/eframe-host/target'/BIN/'lab-eframe-host.exe'
    SOURCE=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    IDENTITIES={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in (RUNTIME,PARENT,CHILD)}
user=c.WinDLL('user32',use_last_error=True)
user.GetWindowThreadProcessId.argtypes=[w.HWND,c.POINTER(w.DWORD)]
user.PostMessageW.argtypes=[w.HWND,w.UINT,w.WPARAM,w.LPARAM]
user.PostMessageW.restype=w.BOOL

def write(path,value): path.write_text(json.dumps(value,indent=2,ensure_ascii=True)+'\n',encoding='utf-8')
def wait(predicate,label,timeout=20):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        value=predicate()
        if value:return value
        time.sleep(.02)
    raise AssertionError('deadline: '+label)

class Process:
    def __init__(self,name,args):
        self.lines=[]
        self.file=(OUT/(name+'.log')).open('wb')
        self.p=subprocess.Popen([str(a) for a in args],cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,creationflags=subprocess.CREATE_NO_WINDOW)
        def drain():
            for line in self.p.stdout:
                self.file.write(line);self.file.flush()
                try:self.lines.append(json.loads(line))
                except (ValueError,UnicodeDecodeError):pass
                assert len(self.lines)<4096
        self.thread=threading.Thread(target=drain,daemon=True);self.thread.start()
    def finish(self):
        self.thread.join(3)
        if not self.thread.is_alive():self.file.close()

class Relay:
    def __init__(self,address):
        self.address=address;self.stop=False;self.connections=0;self.requests=[];self.errors=[];self.sockets=[]
        self.listener=socket.socket();self.listener.bind(('127.0.0.1',0));self.listener.listen(2)
        self.port=self.listener.getsockname()[1]
        self.thread=threading.Thread(target=self.run,daemon=True);self.thread.start()
    def run(self):
        try:
            while not self.stop:
                if not select.select([self.listener],[],[],.05)[0]:continue
                peer,_=self.listener.accept();self.connections+=1
                upstream=socket.create_connection(self.address,timeout=2)
                self.sockets.extend([peer,upstream]);pending=b''
                with (OUT/'runtime-client-wire.ndjson').open('ab') as transcript:
                    while not self.stop:
                        readable=select.select([peer,upstream],[],[],.05)[0]
                        ended=False
                        for origin in readable:
                            data=origin.recv(65536)
                            if not data:ended=True;break
                            if origin is peer:
                                transcript.write(data);transcript.flush();pending+=data
                                assert len(pending)<=1048576
                                while b'\n' in pending:
                                    line,pending=pending.split(b'\n',1)
                                    self.requests.append(json.loads(line))
                                    assert len(self.requests)<=1024
                            (upstream if origin is peer else peer).sendall(data)
                        if ended:break
                peer.close();upstream.close()
        except OSError as error:
            if not self.stop:self.errors.append(str(error))
        finally:
            for stream in self.sockets:
                try:stream.close()
                except OSError:pass
    def close(self):
        self.stop=True
        for stream in self.sockets:
            try:stream.shutdown(socket.SHUT_RDWR)
            except OSError:pass
        self.thread.join(3);self.listener.close()

class API:
    def __init__(self,address):
        host,port=address.split(':');self.socket=socket.create_connection((host,int(port)),timeout=5)
        self.socket.settimeout(5);self.reader=self.socket.makefile('rb');self.next=0;self.transcript=[]
        self.call('hello',{})
    def call(self,op,args):
        self.next+=1;id=str(self.next);request=dict(v=1,type='request',call_id=id,op=op,args=args)
        self.transcript.append(dict(direction='request',message=request))
        self.socket.sendall(json.dumps(request).encode()+b'\n')
        while True:
            response=json.loads(self.reader.readline(1048577));self.transcript.append(dict(direction='response',message=response))
            assert len(self.transcript)<=4096
            if response.get('call_id')==id:
                assert response['type']=='result',response
                return response['result']
    def close(self):self.reader.close();self.socket.close()

def document(title,target,missing=False):
    scalar=dict(target)
    if missing:scalar['parameter']='missing-eframe-2-test'
    return dict(format_version=1,document_id='eframe-2-real-parent',controls=[],windows=[dict(id='window',title=title,tabs=[dict(id='tab',title='Live Runtime observations',panels=[
        dict(id='scalar-panel',title='Scalar with original unit',kind=dict(kind='status',source=scalar)),
        dict(id='plot-panel',title='Live Plot',kind=dict(kind='plot',plot_id='plot'))])])],plots=[dict(id='plot',title='Runtime measurements',time_window_seconds=60.,axes=dict(y_min=None,y_max=None),traces=[dict(id='trace',source=target,display_label='Virtual measurement',visible=True,style=dict(color='cyan',width=2.),display_unit=None)])])

def read_json(path):
    try:return json.loads(path.read_bytes())
    except (OSError,ValueError):return None

def complete_png(path):
    try:
        data=path.read_bytes()
        return data.startswith(b'\x89PNG\r\n\x1a\n') and data.endswith(b'\x00\x00\x00\x00IEND\xaeB`\x82')
    except OSError:return False

def verify_png(path):
    data=path.read_bytes();assert complete_png(path)
    offset=8;compressed=b'';width=height=None
    while offset<len(data):
        length=struct.unpack('>I',data[offset:offset+4])[0];kind=data[offset+4:offset+8];payload=data[offset+8:offset+8+length]
        assert zlib.crc32(kind+payload)&0xffffffff==struct.unpack('>I',data[offset+8+length:offset+12+length])[0]
        if kind==b'IHDR':width,height,depth,color,_,_,interlace=struct.unpack('>IIBBBBB',payload);assert depth==8 and color==6 and interlace==0
        if kind==b'IDAT':compressed+=payload
        offset+=length+12
    assert 0<width<=3200 and 0<height<=2000
    raw=zlib.decompress(compressed);stride=width*4;prior=bytearray(stride);pos=0;blue=bright=0
    for y in range(height):
        filter=raw[pos];pos+=1;row=bytearray(raw[pos:pos+stride]);pos+=stride
        for x in range(stride):
            left=row[x-4] if x>=4 else 0;up=prior[x];corner=prior[x-4] if x>=4 else 0
            if filter==1:row[x]=(row[x]+left)&255
            elif filter==2:row[x]=(row[x]+up)&255
            elif filter==3:row[x]=(row[x]+(left+up)//2)&255
            elif filter==4:
                estimate=left+up-corner;a=abs(estimate-left);b=abs(estimate-up);d=abs(estimate-corner)
                row[x]=(row[x]+(left if a<=b and a<=d else up if b<=d else corner))&255
            else:assert filter==0
        for x in range(0,stride,4):
            r,g,b=row[x:x+3];bright+=r>180 and g>180 and b>180
            blue+=y>250 and b>r+15 and g>r+10 and b>80
        prior=row
    receipt=read_json(path.with_suffix('.json'))
    expected_curve=any(plot['points']>=2 for plot in receipt['display']['plots'])
    assert bright>100 and (not expected_curve or blue>30),(path,blue,bright,receipt)
    return dict(path=str(path),width=width,height=height,blue_plot_pixels=blue,bright_pixels=bright,sha256=hashlib.sha256(data).hexdigest())

def receipts(directory,predicate):
    for path in sorted(directory.glob('frame-*.json')):
        value=read_json(path)
        if value and predicate(value) and complete_png(path.with_suffix('.png')):return path,value
    return None

def original_window_close(ready):
    pid=w.DWORD();user.GetWindowThreadProcessId(ready['hwnd'],c.byref(pid));assert pid.value==ready['pid']
    assert user.PostMessageW(ready['hwnd'],0x10,0,0),c.get_last_error()

if __name__ == "__main__":
    runtime=parent=relay=api=None
    result=dict(source=SOURCE,profile=PROFILE,renderer=RENDERER,scenario=SCENARIO,binaries=IDENTITIES,activation_qualified=False,passed=False)
    try:
        config=(ROOT/'examples/runtime.minimal.toml').read_text(encoding='utf-8').replace('path = "history.sqlite"','path = "'+str(OUT/'history.sqlite').replace('\\','/')+'"')
        (OUT/'runtime.toml').write_text(config,encoding='utf-8')
        runtime=Process('runtime',[RUNTIME,'--serve','--config',OUT/'runtime.toml'])
        ready=wait(lambda:next((line for line in runtime.lines if line.get('state')=='ready' and 'port'in line),None),'real Runtime readiness')
        relay=Relay(('127.0.0.1',int(ready['port'])))
        parent=Process('parent',[PARENT,'--connect',f'127.0.0.1:{relay.port}','--workspace',OUT/'workspace','--workbench-listen','127.0.0.1:0',
            '--experimental-eframe',RENDERER,'--experimental-eframe-child',CHILD,'--experimental-eframe-commit-limit','1073741824',
            '--experimental-eframe-restart-limit','1','--experimental-eframe-capture'])
        endpoint=wait(lambda:next((line['workbench_endpoint'] for line in parent.lines if 'workbench_endpoint'in line),None),'Workbench API')
        api=API(endpoint)
        status=wait(lambda:(lambda s:s if s['runtime_client']['freshness']=='fresh' else None)(api.call('client_status',{})),'authoritative rebuild')
        workbench_id=status['workbench_id'];scope=status['runtime_client']['hello']['scope']
        before=api.call('presentation_get',{})
        source=before['document']['plots'][0]['traces'][0]['source']
        def replace(title,missing=False):
            current=api.call('presentation_get',{})
            revision=current.get('presentation_revision',current.get('revision'))
            reply=api.call('ui_replace_document',dict(expected=dict(workbench_id=workbench_id,revision=revision),document=document(title,source,missing)))
            return int(reply['presentation_revision'])
        revision=replace('EFRAME-2 real Runtime live')
        directory=CHILD.parent/'target/eframe-2-parent'/f'{parent.p.pid}-{workbench_id}-1'
        child_ready=wait(lambda:read_json(directory/'ready.json'),'first real window')
        first_path,first=wait(lambda:receipts(directory,lambda value:value['document_revision']==revision and value['display']['scalars'] and value['display']['scalars'][0]['freshness']=='Fresh' and value['display']['plots'] and value['display']['plots'][0]['points']>=2),'live scalar/plot pixels')
        second_path,second=wait(lambda:receipts(directory,lambda value:value['document_revision']==revision and value['display']['plots'] and value['display']['plots'][0]['last']!=first['display']['plots'][0]['last'] and value['display']['plots'][0]['points']>first['display']['plots'][0]['points']),'changing real measurements')
        missing=replace('EFRAME-2 unavailable selected source',True)
        unavailable=wait(lambda:receipts(directory,lambda value:value['document_revision']==missing and value['display']['scalars'] and value['display']['scalars'][0]['freshness'].startswith('Unavailable')),'unavailable selected source')
        latest=replace('EFRAME-2 API replaced document')
        replaced=wait(lambda:receipts(directory,lambda value:value['document_revision']==latest and value['display']['heading']=='EFRAME-2 API replaced document' and value['display']['plots'] and value['display']['plots'][0]['points']>=2),'API replacement pixels')
        wire_before=len(relay.requests);connections_before=relay.connections
        terminal=restored=stale=None
        directory2=directory;restarted_ready=child_ready
        if SCENARIO=='fault':
            # External test crash, directed at the exact child PID from its own HWND/receipt.
            subprocess.run(['taskkill','/PID',str(child_ready['pid']),'/F'],check=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
            directory2=CHILD.parent/'target/eframe-2-parent'/f'{parent.p.pid}-{workbench_id}-2'
            restarted_ready=wait(lambda:read_json(directory2/'ready.json'),'controlled same-renderer restart')
            restored=wait(lambda:receipts(directory2,lambda value:value['document_revision']==latest and value['display']['heading']=='EFRAME-2 API replaced document' and value['display']['plots'] and value['display']['plots'][0]['points']>=2),'current Parent-approved restoration')
            after=api.call('client_status',{})
            assert after['workbench_id']==workbench_id and after['runtime_client']['hello']['scope']==scope
            assert relay.connections==connections_before==1,(relay.connections,connections_before)
            assert len(relay.requests)==wire_before,(relay.requests[wire_before:])
            assert runtime.p.poll() is None,'Runtime died because of GUI crash'
            terminal=wait(lambda:next((line for line in parent.lines if line.get('incarnation')==1 and line.get('phase')=='ExitedContained'),None),'original exit/IPC retirement')
            assert terminal['objects']==[0,0,0] and terminal['ipc_handles']==0 and terminal['ipc_pending_io']==0 and terminal['ipc_retired']
            # Independently injected virtual Runtime loss supplies the real stale state.
            runtime.p.terminate();runtime.p.wait(timeout=10)
            stale=wait(lambda:receipts(directory2,lambda value:value['display']['scalars'] and value['display']['scalars'][0]['freshness']=='Stale'),'actual Runtime-loss stale display')
        original_window_close(restarted_ready)
        parent_exit=parent.p.wait(timeout=15)
        assert parent_exit==0,parent.lines
        if SCENARIO=='normal':assert runtime.p.poll() is None,'normal GUI close stopped Runtime'
        closed=read_json(directory2/'on-exit.json');returned=read_json(directory2/'returned.json')
        assert closed['on_exit'] and closed['closed_write_completed'] and returned['run_native_returned'] and not returned['error']
        final=wait(lambda:next((line for line in parent.lines if line.get('incarnation')==(2 if SCENARIO=='fault' else 1) and line.get('phase')=='ExitedHealthy'),None),'normal close original exit')
        assert final['objects']==[0,0,0] and final['ipc_retired'] and final['ipc_handles']==0 and final['ipc_pending_io']==0
        forbidden=[r for r in relay.requests if r.get('type')=='mutation' or 'seq'in r or r.get('op') in ('operation_status','exact_retry')]
        assert not forbidden,forbidden
        result.update(workbench_id=workbench_id,scope=scope,runtime_connections_during_gui_fault=connections_before,
            runtime_requests_during_gui_fault=len(relay.requests)-wire_before,requests=relay.requests,first_window=child_ready,restarted_window=restarted_ready,
            captures=[str(first_path),str(second_path),str(unavailable[0]),str(replaced[0])]+([str(restored[0]),str(stale[0])] if SCENARIO=='fault' else []),
            crash_retirement=terminal,normal_retirement=final,child_directories=[str(directory),str(directory2)],parent_exit=parent_exit)
        result['framebuffer_verification']=[verify_png(Path(path).with_suffix('.png')) for path in result['captures']]
        result['passed']=True
    finally:
        if api:
            write(OUT/'workbench-api.json',api.transcript);api.close()
        if parent and parent.p.poll() is None:parent.p.terminate();parent.p.wait(timeout=10)
        if runtime and runtime.p.poll() is None:runtime.p.terminate();runtime.p.wait(timeout=10)
        if relay:relay.close();result['relay_errors']=relay.errors
        if parent:parent.finish()
        if runtime:runtime.finish()
        write(OUT/'result.json',result)
    print(json.dumps(dict(passed=result['passed'],output=str(OUT),profile=PROFILE,renderer=RENDERER)))
