import json,subprocess,time,threading,urllib.parse
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from gi.repository import Gio,GLib
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
DEST='org.mpris.MediaPlayer2.spotify'; PATH='/org/mpris/MediaPlayer2'; IFACE='org.mpris.MediaPlayer2.Player'
lock=threading.Lock(); stopping=threading.Event();active_session=None
def call(method,params=None):
    return bus.call_sync(DEST,PATH,IFACE,method,params,None,Gio.DBusCallFlags.NONE,10000,None)
def status():
    return bus.call_sync(DEST,PATH,'org.freedesktop.DBus.Properties','GetAll',GLib.Variant('(s)',(IFACE,)),None,Gio.DBusCallFlags.NONE,10000,None).unpack()[0]
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_POST(self):
        url=urllib.parse.urlsplit(self.path);path=url.path
        method={'/pause':'Pause','/resume':'Play','/stop':'Pause'}.get(path)
        if not method:self.send_error(404);return
        try:
            if path=='/stop' and urllib.parse.parse_qs(url.query).get('session',[None])[0]!=active_session:
                self.send_response(204);self.end_headers();return
            if path=='/stop': stopping.set()
            call(method)
            if path=='/stop' and lock.acquire(timeout=4):lock.release()
            self.send_response(204);self.end_headers()
        except Exception:self.send_error(503)
    def do_GET(self):
        global active_session
        if self.path=='/health':
            try:
                s=status(); self.send_response(200);self.end_headers();self.wfile.write(json.dumps({'ok':True,'state':s.get('PlaybackStatus')}).encode())
            except Exception:self.send_error(503)
            return
        url=urllib.parse.urlsplit(self.path);id=url.path.removeprefix('/audio/')
        if not url.path.startswith('/audio/') or len(id)!=22 or not id.isalnum():self.send_error(404);return
        if not lock.acquire(timeout=8):self.send_error(409);return
        capture=None;stopping.clear();active_session=urllib.parse.parse_qs(url.query).get('session',[None])[0]
        try:
            duration=int(urllib.parse.parse_qs(url.query)['duration'][0])
            if not 0<duration<=14400000:raise ValueError()
            call('OpenUri',GLib.Variant('(s)',('spotify:track:'+id,)))
            deadline=time.monotonic()+20
            while time.monotonic()<deadline:
                s=status();m=s.get('Metadata',{})
                if id in str(m.get('xesam:url','')) and s.get('PlaybackStatus')=='Playing':break
                time.sleep(.2)
            else:raise RuntimeError('Track did not start')
            call('Pause')
            trackpath=s['Metadata']['mpris:trackid']
            call('SetPosition',GLib.Variant('(ox)',(trackpath,0)))
            capture=subprocess.Popen(['ffmpeg','-loglevel','error','-f','pulse','-i','elvnx_spotify.monitor','-ac','2','-ar','48000','-f','s16le','pipe:1'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
            call('Play');self.send_response(200);self.send_header('Content-Type','application/octet-stream');self.end_headers()
            last=0;paused=False;sent=0
            while not stopping.is_set():
                data=capture.stdout.read(3840)
                if not data:break
                now=time.monotonic()
                if now-last>.2:
                    s=status();last=now;paused=s.get('PlaybackStatus')!='Playing'
                    if id not in str(s.get('Metadata',{}).get('xesam:url','')) or s.get('PlaybackStatus')=='Stopped':break
                if paused:continue
                self.wfile.write(data);sent+=len(data)
                if sent>=duration*192:break
        except (BrokenPipeError,ConnectionResetError):pass
        except Exception as e:
            print('Spotify bridge:',type(e).__name__,str(e)[:150],flush=True)
        finally:
            if capture:
                capture.terminate()
                try:capture.wait(timeout=2)
                except subprocess.TimeoutExpired:capture.kill();capture.wait(timeout=2)
            try:call('Pause')
            except Exception:pass
            active_session=None;lock.release()
ThreadingHTTPServer(('127.0.0.1',3680),Handler).serve_forever()
