"""Local model review UI; native geometry, skinning, textures and material shader.

python scripts/model-review/viewer.py --batch   # representative views to captures
python scripts/model-review/viewer.py           # http://127.0.0.1:8768
"""
import argparse
import atexit
import io
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time
import re
from urllib.parse import urlsplit
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "captures/rift-texture-review"
# The "candidate" pack the renderer draws; imagegen_wire.py's worker lanes point it at their own copy.
CANDIDATE = OUT / "candidate/0-riftapart"
sys.path.insert(0, str(ROOT / "scripts"))
import uyactl

MODELS = json.loads(Path(__file__).with_name('review-assets.json').read_text(encoding='utf-8'))
asset_jobs={}
asset_job_lock=threading.Lock()


def prepare_asset(kind, key):
    """One bounded, local extraction job, validated against the installed index."""
    if kind not in ('models','textures') or not re.fullmatch(r'[0-9a-f]{16}',key):
        raise ValueError('Expected a catalogued model or texture ID')
    catalogue=json.loads((OUT/'asset-catalogue.json').read_text())
    if not any(r['id']==key for r in catalogue['sources'][kind]):raise ValueError('Unknown asset')
    with asset_job_lock:
        if any(j['state']=='running' for j in asset_jobs.values()):raise ValueError('A source export is already running')
        job=kind+'-'+key
        asset_jobs[job]={'state':'running'}
    def work():
        try:
            script='inspect_rift_models.py' if kind=='models' else 'export_texture.py'
            log=OUT/('library-'+job+'.log')
            # The only interpolated value is the hex ID checked above; all paths
            # and resource limits are fixed by this local application.
            command=f"& ./scripts/watch-run.ps1 -Exe python -ArgLine 'scripts/model-review/{script} --id {key}' -Log captures/rift-texture-review/library-{job}.log -Seconds 240 -LimitMB 2800 -MinimumFreeGB 3"
            result=subprocess.run(['powershell','-NoProfile','-Command',command],cwd=ROOT,
                                  capture_output=True,text=True,creationflags=subprocess.CREATE_NO_WINDOW)
            guard=Path(str(log)+'.watch')
            if result.returncode or not guard.exists() or 'exit 0,' not in guard.read_text():
                error=Path(str(log)+'.err')
                raise RuntimeError((error.read_text(errors='replace') if error.exists() else result.stderr)[-2000:])
            target=OUT/'library'/kind/key
            report=target/('source-inspection.json' if kind=='models' else 'slice0.json')
            if not report.exists():raise RuntimeError('Export did not produce its source report')
            details=json.loads(report.read_text())
            if kind=='models':details={'models':details['models'],'source_report':str(report)}
            asset_jobs[job]={'state':'ready','message':'Local source is ready for inspection. It has not replaced a game asset.',
                             'report':json.dumps(details,indent=2)}
        except Exception as error:asset_jobs[job]={'state':'failed','error':str(error)}
    threading.Thread(target=work,daemon=True).start()
    return job


class Renderer:
    def __init__(self, args):
        self.args = args
        self.process = None
        self.render_lock = threading.Lock()
        self.pack = None
        self.log = open(OUT / "viewer-native.log", "a", encoding="utf-8")
        self.last_activity = time.monotonic()
        self.peak = 0
        atexit.register(self.close)

    def close(self):
        if self.process and self.process.poll() is None:
            try:
                self.process.stdin.write("quit\n")
                self.process.stdin.flush()
                self.process.wait(timeout=3)
            except (OSError, subprocess.TimeoutExpired):
                self.process.kill()
                self.process.wait(timeout=3)
        self.process = None

    def start(self, pack, level):
        self.close()
        rift = CANDIDATE if pack == "candidate" else OUT / "profile/texture_replacements/0-riftapart"
        if not rift.is_dir():
            raise RuntimeError(f"Pack not available: {rift}")
        exe = OUT / "viewer-build/uya-model-review.exe"
        self.answers = queue.Queue()
        self.process = subprocess.Popen([str(exe), str(ROOT / "disc/SCUS_973.53.iso"), str(level), str(self.args.height),
                                        str(OUT / "profile/texture_replacements"), str(rift)],
                                       cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       stderr=self.log, text=True, encoding="utf-8", errors="replace", bufsize=1,
                                       creationflags=subprocess.CREATE_NO_WINDOW,
                                       env={**os.environ, **({"UYA_REVIEW_EXPORT_OBJ":"1"} if self.args.export_obj else {})})
        process, answers = self.process, self.answers
        def reader():
            for line in process.stdout:
                if line.startswith("REVIEW "):
                    answers.put(line.strip())
                else:
                    self.log.write(line)
                    self.log.flush()
            answers.put("REVIEW ERROR native renderer ended; see viewer-native.log")
        def watch():
            handle = uyactl.K32.OpenProcess(0x1000, False, process.pid)
            try:
                while process.poll() is None:
                    try:
                        mb = uyactl.private_mb(handle)
                    except OSError:
                        break
                    self.peak = max(self.peak, mb)
                    if mb > self.args.limit_mb or time.monotonic()-self.last_activity > 300:
                        answers.put("REVIEW ERROR renderer stopped at memory limit or five minutes idle")
                        process.kill()
                        break
                    time.sleep(1)
            finally:
                uyactl.K32.CloseHandle(handle)
        threading.Thread(target=reader, daemon=True).start()
        threading.Thread(target=watch, daemon=True).start()
        response = self.answers.get(timeout=60)
        if response != "REVIEW READY":
            raise RuntimeError(response)
        self.pack = (pack,level)

    def render(self, params, destination=None):
        # Browsers may keep spare connections open; HTTP serves those separately,
        # while the native request/response stream remains strictly serialized.
        with self.render_lock:
            return self._guarded_render(params, destination)

    def _guarded_render(self, params, destination=None):
        # Batch invocations are already guarded by watch-run.ps1. Interactive
        # requests take the same exclusive slots briefly, so another session's
        # gameplay measurement never shares GPU work with a review redraw.
        held = None
        if not self.args.batch and not self.args.requests:
            resident = self.process and self.process.poll() is None
            held = uyactl.enter_slot("model review redraw", True, .5 if resident else 3)
        try:
            return self._render(params, destination)
        finally:
            if held is not None:
                uyactl.exit_slot(held)

    def _render(self, params, destination=None):
        self.last_activity = time.monotonic()
        pack = params.get("pack", "before")
        if pack not in ("before", "candidate"):
            raise ValueError("Unknown pack")
        model = MODELS[params.get("model", "ranger")]
        required = model.get('candidate_geometry')
        if required and pack == 'before':
            required = 'original'
        if required:
            params = dict(params)
            if params.get('geometry', required) != required:
                raise ValueError('This texture pack requires geometry: ' + required)
            params['geometry'] = required
        if not self.process or self.process.poll() is not None or (pack,model['level']) != self.pack:
            self.start(pack,model['level'])
        poses=sorted((OUT/(self.args.poses or model['poses'])).glob('ram*.bin'))
        if not poses:raise ValueError('This asset has no captured poses')
        index = int(params.get("frame", 0)) % len(poses)
        destination = destination or OUT / "viewer-current.png"
        values = [str(poses[index]), model["prefixes"]]
        values += [str(float(params.get(k, default))) for k, default in
                   (("yaw", 90), ("pitch", 8), ("zoom", 1), ("pan_x", 0), ("pan_y", 0))]
        values += [str(int(params.get("instance", model.get("instance", 0)))), str(destination), "1" if params.get("reload") else "0"]
        channels = {"lit":0,"colour":1,"metalness":2,"roughness":3,"normals":4,"ao":5,"triangle":6,"emission":7,"game":8}
        values += [str(channels[params.get("channel","lit")]), '1' if model['whole'] else '0']
        edited=OUT/'blender/edited'/(params.get('model','ranger')+'.r3edit')
        if params.get('geometry')=='edited' and not edited.is_file():raise ValueError('This model has no Blender export yet')
        source_near=model.get('source_lod')=='near' or params.get('geometry')=='source_near'
        if model.get('source_layer') in ('terrain','shrub','tie','source','body','scene'):
            values += ['@'+model['source_layer']+':'+str(int(model['source_id']))+(',ship' if model.get('ship_assembly') else '')+('|' + str(edited) if params.get('geometry')=='edited' else '')]
        else:
            geometry=params.get('geometry','original')
            values += [('' if not source_near else '@near:')+str(edited) if geometry=='edited' else
                       ('@near:family' if source_near else '@family') if geometry=='family' else '@near' if source_near else '']
        values += [','.join(str(v) for v in model.get('attachment_pair',[0,0]))]
        values += [str(OUT/model['emission_policy']) if model.get('emission_policy') else '']
        self.process.stdin.write("\t".join(values) + "\n")
        self.process.stdin.flush()
        try:
            response = self.answers.get(timeout=60)
        except queue.Empty:
            self.close()
            raise RuntimeError("Native render timed out")
        if response != "REVIEW OK":
            raise RuntimeError(response)
        image = Image.open(destination)
        buffer = io.BytesIO()
        image.save(buffer, "JPEG", quality=95)
        return buffer.getvalue(), None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--batch", action="store_true")
    parser.add_argument("--export-obj", action="store_true", help="Export selected captured geometry and level light sets beside batch renders")
    parser.add_argument("--pack", choices=["before", "candidate"], default="before")
    parser.add_argument("--height", type=int, default=900)
    parser.add_argument("--limit-mb", type=int, default=3500)
    parser.add_argument("--port", type=int, default=8768)
    parser.add_argument("--poses", help='Override the selected asset capture folder')
    parser.add_argument('--assets',type=Path,default=Path(__file__).with_name('review-assets.json'),help='Any named captured-model selection manifest')
    parser.add_argument("--requests", type=Path, help="JSON list of named requests instead of HTTP")
    args = parser.parse_args()
    global MODELS
    MODELS=json.loads(args.assets.read_text(encoding='utf-8'))
    for key,model in MODELS.items():
        if not re.fullmatch(r'[a-z0-9_]+',key):raise ValueError('Invalid asset key')
        if not all(re.fullmatch(r'[0-9a-f]{5,16}',p) for p in model['prefixes'].split(',')):raise ValueError('Invalid texture prefix')
        if not 1<=int(model['level'])<=55:raise ValueError('Invalid level')
        folder=(OUT/(args.poses or model['poses'])).resolve()
        if not folder.is_relative_to((ROOT/'captures').resolve()):raise ValueError('Capture folder outside repository captures')
        model['frames']=len(list(folder.glob('ram*.bin')))
        if not model['frames']:raise ValueError('No recorded poses for '+key)
    renderer = Renderer(args)
    if args.batch or args.requests:
        requests = json.loads(args.requests.read_text()) if args.requests else [
            {"name": f"{model}-{yaw}", "model": model, "yaw": yaw, "pack": args.pack}
            for model in MODELS for yaw in (0, 90, 180, 270)]
        records = []
        for request in requests:
            request.setdefault("pack", args.pack)
            name = request.get("name", "frame")
            if Path(name).name != name:
                raise ValueError("Capture name must be a filename")
            path = OUT / f"{args.pack}-{name}.png"
            try:
                jpeg, _ = renderer.render(request, path)
                path.with_suffix(".jpg").write_bytes(jpeg)
                records.append({"request": request, "path": str(path)})
                print(f"{name}: captured", flush=True)
            except Exception as error:
                records.append({"request": request, "error": str(error)})
                print(f"{name}: {error}", flush=True)
        (OUT / f"{args.pack}-viewer-results.json").write_text(json.dumps(records, indent=2))
        renderer.close()
        print(f"Native peak private memory: {renderer.peak:.0f} MB", flush=True)
        if any("error" in record for record in records):
            raise SystemExit(1)
        return

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            self.path=urlsplit(self.path).path
            if self.path == "/":
                body = (Path(__file__).with_name("viewer.html")).read_bytes()
                mime = "text/html; charset=utf-8"
            elif self.path == '/assets':
                body=Path(__file__).with_name('asset_library.html').read_bytes()
                mime='text/html; charset=utf-8'
            elif self.path == '/catalogue':
                catalogue=json.loads((OUT/'asset-catalogue.json').read_text())
                for kind,pattern in [('models','*/*.glb'),('textures','*/slice0.png')]:
                    exported={p.parent.name for p in (OUT/'library'/kind).glob(pattern)}
                    for entry in catalogue['sources'][kind]:entry['exported']=entry['id'] in exported
                body=json.dumps(catalogue).encode();mime='application/json'
            elif self.path.startswith('/source-texture/'):
                key=self.path.rsplit('/',1)[-1]
                catalogue=json.loads((OUT/'asset-catalogue.json').read_text())
                entry=next((r for r in catalogue['sources']['textures'] if r['id']==key),None)
                raw=OUT/'library/textures'/key/'slice0.png'
                if not entry or not (raw.is_file() or entry['preview']):
                    self.send_error(404,'Texture has not been exported');return
                source=(raw if raw.is_file() else Path(entry['preview'])).resolve()
                if not (source.is_relative_to(Path('C:/riftapart-export').resolve()) or source.is_relative_to((OUT/'library/textures').resolve())):
                    self.send_error(400);return
                preview=Image.open(source).convert('RGBA');preview.thumbnail((768,768))
                buffer=io.BytesIO();preview.save(buffer,'PNG');body=buffer.getvalue();mime='image/png'
            elif self.path.startswith('/asset-job/'):
                job=asset_jobs.get(self.path.rsplit('/',1)[-1])
                if job is None:self.send_error(404);return
                body=json.dumps(job).encode();mime='application/json'
            elif self.path.startswith('/target-texture/'):
                key=self.path.rsplit('/',1)[-1]
                if not re.fullmatch(r'[0-9a-f]{16}-[0-9a-f]{16}-[0-9a-f]{8}',key):self.send_error(400);return
                paths=[OUT/'candidate/0-riftapart/replacements'/(key+'.png'),
                       Path.home()/'Games/Ratchet and Clank 3/texture_dumps'/(key+'.png')]
                source=next((p for p in paths if p.is_file()),None)
                if source is None:self.send_error(404);return
                preview=Image.open(source).convert('RGBA');preview.thumbnail((768,768))
                buffer=io.BytesIO();preview.save(buffer,'PNG');body=buffer.getvalue();mime='image/png'
            elif self.path.startswith('/model-export/'):
                key=self.path.rsplit('/',1)[-1]
                if not re.fullmatch(r'[0-9a-f]{16}',key):self.send_error(400);return
                paths=list((OUT/'library/models'/key).glob('*.glb'))
                if len(paths)!=1:self.send_error(404);return
                body=paths[0].read_bytes();mime='model/gltf-binary'
            elif self.path == "/config":
                body = json.dumps({"models": MODELS,"edited":[k for k in MODELS if (OUT/'blender/edited'/(k+'.r3edit')).is_file()],
                                   "candidate": (OUT / "candidate/0-riftapart").is_dir()}).encode()
                mime = "application/json"
            else:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Type", mime)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            if self.path=='/prepare-asset':
                try:
                    if int(self.headers.get('Content-Length','0'))>1024:raise ValueError('Request too large')
                    origin=self.headers.get('Origin')
                    if origin and urlsplit(origin).netloc!=self.headers.get('Host'):raise ValueError('Use the local library page')
                    params=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                    job=prepare_asset(params['kind'],params['id'])
                    body=json.dumps({'job':job}).encode();self.send_response(202)
                except Exception as error:
                    body=json.dumps({'error':str(error)}).encode();self.send_response(400)
                self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
                return
            if self.path != "/render" or int(self.headers.get("Content-Length", "0")) > 4096:
                self.send_error(400)
                return
            try:
                params = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                body, _ = renderer.render(params)
                self.send_response(200)
                self.send_header("Content-Type", "image/jpeg")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            except Exception as error:
                self.send_error(500, str(error))

        def log_message(self, *args):
            pass

    print(f"Model review: http://127.0.0.1:{args.port}", flush=True)
    try:
        ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
    finally:
        renderer.close()


if __name__ == "__main__":
    main()
