"""Spike for sidevoice-engine#36: what an MLX backend through a separate Python mlx-audio process costs on macOS.

Measures only; nothing here is engine code. On an Apple-silicon Mac (CI: GitHub-hosted macos-14) it:

1. downloads a standalone CPython (python-build-standalone, pinned by digest) and unpacks it into a directory of its
   own, the way an app would install a runtime on demand;
2. downloads the hash-pinned wheels of requirements.txt and installs them into that same directory, offline;
3. measures download size, on-disk size and file count, and the size of the result as a single archive;
4. times the interpreter's and the runner's cold start (imports of mlx and mlx-audio);
5. fetches the models at pinned revisions, then runs bench.py on the GPU and on the CPU: load time and time per
   call for Whisper (STT) and Kokoro (TTS, English and Spanish), transcripts, audio durations, real-time factor;
6. records how the shipped binaries are signed, and what happens when the runtime carries a quarantine flag.

Runs on the macOS system Python (3.9) with the standard library only:

    python3 spikes/mlx-python/spike.py --out DIR

It writes DIR/results.json and DIR/results.md, and appends the latter to $GITHUB_STEP_SUMMARY when set.
"""

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent

PYTHON_URL = (
    "https://github.com/astral-sh/python-build-standalone/releases/download/20261003/"
    "cpython-3.12.15%2B20261003-aarch64-apple-darwin-install_only_stripped.tar.gz"
)
PYTHON_SHA256 = "ad8d0c637c0a36b967b310e2c07254f4d2ca8cabaa7699e55ed6290aceb481a2"

# John F. Kennedy's inaugural address (1961), 11 s, 16 kHz mono: a work of the US federal government, in the public
# domain. Taken from whisper.cpp's samples at a fixed commit.
CLIP_URL = (
    "https://raw.githubusercontent.com/ggml-org/whisper.cpp/d1be6fde11ac6e0407606b4e42fe72d34add8037/samples/jfk.wav"
)
CLIP_SHA256 = "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e"

# Hugging Face repositories at fixed revisions.
MODELS = [
    "mlx-community/whisper-small-asr-fp16@42fc41c2ab1c78e844fd01477cf398eec6801c47",
    "mlx-community/whisper-large-v3-turbo-asr-fp16@624c19c9af5603fa73b83bce14d4aeea96156d18",
    "mlx-community/Kokoro-82M-bf16@a71e4d38b236d968966a2002c4c895dbd12b1c3c",
]
WHISPER = ["whisper-small-asr-fp16", "whisper-large-v3-turbo-asr-fp16"]

results = {"events": []}


def log(message):
    print(f"[spike] {message}", flush=True)


def record(kind, **fields):
    event = {"kind": kind, **fields}
    results["events"].append(event)
    log(json.dumps(event, ensure_ascii=False))
    return event


def run(cmd, env=None, timeout=None, check=True):
    start = time.perf_counter()
    proc = subprocess.run(cmd, env=env, timeout=timeout, capture_output=True, text=True)
    seconds = time.perf_counter() - start
    if check and proc.returncode != 0:
        sys.stderr.write(proc.stdout + proc.stderr)
        raise SystemExit(f"command failed ({proc.returncode}): {' '.join(map(str, cmd))}")
    return proc, seconds


def download(url, target, sha256):
    start = time.perf_counter()
    digest = hashlib.sha256()
    with urllib.request.urlopen(url) as response, open(target, "wb") as out:
        while True:
            chunk = response.read(1 << 20)
            if not chunk:
                break
            digest.update(chunk)
            out.write(chunk)
    seconds = time.perf_counter() - start
    if digest.hexdigest() != sha256:
        raise SystemExit(f"digest mismatch for {url}: {digest.hexdigest()}")
    return seconds


def tree(path):
    """Bytes on disk (allocated blocks, as `du`), apparent bytes, and regular files under path."""
    disk = apparent = files = 0
    for root, _dirs, names in os.walk(path):
        for name in names:
            st = os.lstat(os.path.join(root, name))
            if os.path.islink(os.path.join(root, name)):
                continue
            files += 1
            apparent += st.st_size
            disk += st.st_blocks * 512
    return {"disk_bytes": disk, "apparent_bytes": apparent, "files": files}


def biggest(site_packages, n=12):
    sizes = []
    for entry in site_packages.iterdir():
        if entry.is_dir() and not entry.name.endswith((".dist-info", "__pycache__")):
            sizes.append((tree(entry)["disk_bytes"], entry.name))
    return [{"package_dir": name, "disk_bytes": size} for size, name in sorted(sizes, reverse=True)[:n]]


def host():
    def out(cmd):
        return subprocess.run(cmd, capture_output=True, text=True).stdout.strip()

    record(
        "host",
        machine=platform.machine(),
        macos=out(["sw_vers", "-productVersion"]),
        cpu=out(["sysctl", "-n", "machdep.cpu.brand_string"]),
        cores=out(["sysctl", "-n", "hw.ncpu"]),
        memory_bytes=int(out(["sysctl", "-n", "hw.memsize"]) or 0),
        gpu=out(["system_profiler", "SPDisplaysDataType"]),
    )


def install_runtime(out):
    runtime = out / "runtime"
    archive = out / "downloads" / "python.tar.gz"
    archive.parent.mkdir(parents=True)
    seconds = download(PYTHON_URL, archive, PYTHON_SHA256)
    with tarfile.open(archive) as tar:
        tar.extractall(runtime)
    python = runtime / "python" / "bin" / "python3"
    proc, _ = run([python, "-I", "-c", "import sys; print(sys.version)"])
    record(
        "python_download",
        url=PYTHON_URL,
        bytes=archive.stat().st_size,
        seconds=round(seconds, 2),
        version=proc.stdout.strip(),
        **tree(runtime),
    )

    wheels = out / "downloads" / "wheels"
    pip = [python, "-I", "-m", "pip", "--disable-pip-version-check", "--no-cache-dir"]
    _, seconds = run(pip + ["download", "--require-hashes", "-r", HERE / "requirements.txt", "-d", wheels])
    names = sorted(p.name for p in wheels.iterdir())
    record(
        "wheels_download",
        seconds=round(seconds, 2),
        bytes=sum(p.stat().st_size for p in wheels.iterdir()),
        count=len(names),
        not_wheels=[n for n in names if not n.endswith(".whl")],
    )

    # Offline, from what was just downloaded and verified: what an installer would do with a pinned set.
    _, seconds = run(pip + ["install", "--no-index", "--find-links", wheels, "--no-deps"] + sorted(wheels.iterdir()))
    site = next((runtime / "python" / "lib").glob("python3.*/site-packages"))
    record("runtime_installed", install_seconds=round(seconds, 2), biggest=biggest(site), **tree(runtime))

    # The same runtime shipped as one archive instead (bsdtar, as macOS has it).
    for flag, name in (("-z", "gzip"), ("-J", "xz")):
        target = out / "downloads" / f"runtime.tar.{name}"
        _, seconds = run(["tar", "-c", flag, "-f", target, "-C", runtime, "python"])
        record("runtime_archive", compression=name, bytes=target.stat().st_size, seconds=round(seconds, 1))
    return python


def licences(python):
    proc, _ = run([python, "-I", HERE / "bench.py", "licences"])
    collect(proc)


def cold_start(python):
    for label, cmd in (
        ("interpreter", [python, "-I", "-c", "pass"]),
        ("runner_ready", [python, "-I", HERE / "bench.py", "ready"]),
    ):
        for attempt in range(1, 4):
            proc, seconds = run(cmd)
            inner = json.loads(proc.stdout.strip().splitlines()[-1]) if label == "runner_ready" else {}
            record(
                "cold_start",
                what=label,
                attempt=attempt,
                wall_s=round(seconds, 3),
                in_process_s=inner.get("seconds_since_interpreter_start"),
            )


def bench_env(out, offline):
    env = dict(os.environ)
    env.update(HF_HOME=str(out / "hf-home"), HF_HUB_DISABLE_TELEMETRY="1", TOKENIZERS_PARALLELISM="false")
    if offline:
        env["HF_HUB_OFFLINE"] = "1"
    return env


def collect(proc):
    for line in proc.stdout.splitlines():
        if line.startswith("{"):
            event = json.loads(line)
            record(event.pop("kind"), **event)


def bench(python, out):
    clip = out / "downloads" / "jfk.wav"
    download(CLIP_URL, clip, CLIP_SHA256)
    models = out / "models"
    cmd = [python, "-I", HERE / "bench.py", "fetch", "--models", models]
    for spec in MODELS:
        cmd += ["--repo", spec]
    proc, _ = run(cmd, env=bench_env(out, offline=False))
    collect(proc)

    for device in ("gpu", "cpu"):
        cmd = [
            python,
            "-I",
            HERE / "bench.py",
            "run",
            "--models",
            models,
            "--device",
            device,
            "--clip",
            clip,
            "--out",
            out / "audio",
        ]
        for name in WHISPER:
            cmd += ["--whisper", name]
        proc, seconds = run(cmd, env=bench_env(out, offline=True), timeout=3600, check=False)
        collect(proc)
        record(
            "bench_process",
            device=device,
            exit_code=proc.returncode,
            wall_s=round(seconds, 1),
            stderr_tail=proc.stderr[-4000:],
        )


def signing(python, out):
    runtime = out / "runtime" / "python"
    site = next((runtime / "lib").glob("python3.*/site-packages"))
    targets = [Path(os.path.realpath(python)), next((runtime / "lib").glob("libpython3*.dylib"), None)]
    targets += sorted(site.glob("mlx/lib/*.dylib")) + sorted(site.glob("mlx/core*.so"))
    targets += sorted(site.glob("mlx/lib/*.metallib")) + sorted(site.glob("espeakng_loader/*.dylib"))
    for target in filter(None, targets):
        proc, _ = run(["codesign", "-dv", "--verbose=2", target], check=False)
        record(
            "codesign",
            file=str(target.relative_to(out)),
            exit_code=proc.returncode,
            detail=[
                l
                for l in proc.stderr.splitlines()
                if l.split("=")[0] in ("Signature", "Authority", "TeamIdentifier", "Format", "CodeDirectory v", "flags")
                or "not signed" in l
                or l.startswith("Signature")
            ],
        )
    proc, _ = run(["spctl", "--assess", "--type", "execute", "-vv", python], check=False)
    record("spctl", file="runtime/python/bin/python3", exit_code=proc.returncode, output=proc.stderr.strip())

    # As if a browser or a quarantine-enabled app had downloaded it: flag every file, then run it.
    qflag = f"0083;{int(time.time()):x};spike;"
    run(["xattr", "-r", "-w", "com.apple.quarantine", qflag, runtime], check=False)
    proc, seconds = run(
        [python, "-I", "-c", "import mlx.core as mx; print(mx.default_device())"], check=False, timeout=300
    )
    record(
        "quarantined_run",
        exit_code=proc.returncode,
        wall_s=round(seconds, 2),
        stdout=proc.stdout.strip(),
        stderr=proc.stderr.strip()[-2000:],
    )
    proc, _ = run(["spctl", "--assess", "--type", "execute", "-vv", python], check=False)
    record("spctl_quarantined", exit_code=proc.returncode, output=proc.stderr.strip())


def mib(n):
    return f"{n / (1 << 20):.1f} MiB"


def markdown():
    ev = results["events"]

    def first(kind, **match):
        return next((e for e in ev if e["kind"] == kind and all(e.get(k) == v for k, v in match.items())), None)

    lines = ["## Runtime", "", "| What | Value |", "|---|---|"]
    py, wh, inst = first("python_download"), first("wheels_download"), first("runtime_installed")
    if py:
        lines.append(f"| Python download (`{py['version'].split()[0]}`, install_only_stripped) | {mib(py['bytes'])} |")
        lines.append(f"| Python unpacked | {mib(py['disk_bytes'])}, {py['files']} files |")
    if wh:
        lines.append(f"| Wheels download ({wh['count']} files) | {mib(wh['bytes'])}; not wheels: {wh['not_wheels']} |")
    if py and wh:
        lines.append(f"| Total download, Python + wheels | {mib(py['bytes'] + wh['bytes'])} |")
    if inst:
        lines.append(f"| Installed runtime on disk | {mib(inst['disk_bytes'])}, {inst['files']} files |")
        top = ", ".join(f"{b['package_dir']} {mib(b['disk_bytes'])}" for b in inst["biggest"][:8])
        lines.append(f"| Largest packages | {top} |")
    for e in (x for x in ev if x["kind"] == "runtime_archive"):
        lines.append(f"| Runtime as one `.tar.{e['compression']}` | {mib(e['bytes'])} |")
    for what in ("interpreter", "runner_ready"):
        runs = [e for e in ev if e["kind"] == "cold_start" and e["what"] == what]
        if runs:
            lines.append(f"| Cold start, {what} (wall, 3 runs) | {', '.join(str(e['wall_s']) + ' s' for e in runs)} |")
    for e in (x for x in ev if x["kind"] == "model_fetch"):
        lines.append(f"| Model `{e['repo']}` | {mib(e['bytes'])}, fetched in {e['seconds']} s |")

    lines += [
        "",
        "## Device",
        "",
        "| Device | default_device | Metal | matmul 2048² | bench exit | peak RSS | MLX peak |",
        "|---|---|---|---|---|---|---|",
    ]
    for device in ("gpu", "cpu"):
        d, m = first("device", requested=device), first("matmul", device=device)
        b, mem = first("bench_process", device=device), first("memory", device=device)
        if d or b:
            lines.append(
                f"| {device} | {d and d['default_device']} | {d and d['metal_available']} | "
                f"{m and str(m['gflops']) + ' GFLOP/s'} | {b and b['exit_code']} | "
                f"{mem and mib(mem['peak_rss_bytes'])} | {mem and mib(mem['mlx_peak_bytes'])} |"
            )

    lines += [
        "",
        "## Models",
        "",
        "| Device | Model | Input | Load s | Audio s | 1st call s | Warm call s | RTF 1st | RTF warm | Output |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for e in ev:
        if e["kind"] in ("tts", "stt"):
            load = first("load", device=e["device"], model=e["model"])
            text = e.get("transcript") if e["kind"] == "stt" else e.get("text")
            label = e.get("input") or f"TTS {e['case']} ({e['voice']})"
            if e.get("decoding") == "greedy":
                label += " (greedy)"
            lines.append(
                f"| {e['device']} | {e['model']} | {label} | {load and load['seconds']} | {e['audio_s']} | "
                f"{e['first_call_s']} | {e['warm_median_s']} | {e['rtf_first']} | {e['rtf_warm']} | "
                f"{(text or '').replace('|', '/')} |"
            )

    lines += ["", "## Licences declared by the installed distributions", "", "| Distribution | Declared |", "|---|---|"]
    for e in (x for x in ev if x["kind"] == "licence"):
        declared = e["expression"] or "; ".join(e["classifiers"]) or e["license_field"] or "nothing declared"
        lines.append(f"| {e['name']} {e['version']} | {declared.replace('|', '/')} |")

    lines += ["", "## Signing", "", "| File | codesign |", "|---|---|"]
    for e in (x for x in ev if x["kind"] == "codesign"):
        lines.append(f"| `{e['file']}` | {'; '.join(e['detail']) or 'exit ' + str(e['exit_code'])} |")
    for kind in ("spctl", "spctl_quarantined", "quarantined_run"):
        e = first(kind)
        if e:
            detail = e.get("output") or f"stdout `{e.get('stdout')}`, stderr `{(e.get('stderr') or '')[-300:]}`"
            lines.append(f"| {kind} (exit {e['exit_code']}) | {detail.replace(chr(10), ' ')} |")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    try:
        host()
        python = install_runtime(out)
        licences(python)
        cold_start(python)
        bench(python, out)
        signing(python, out)
    finally:
        (out / "results.json").write_text(json.dumps(results, indent=2, ensure_ascii=False))
        md = markdown()
        (out / "results.md").write_text(md)
        summary = os.environ.get("GITHUB_STEP_SUMMARY")
        if summary:
            with open(summary, "a") as f:
                f.write(md)
        print(md)
    failed = [e["device"] for e in results["events"] if e["kind"] == "bench_process" and e["exit_code"] != 0]
    if failed:
        raise SystemExit(f"bench failed on: {', '.join(failed)} (stderr in results.json)")


if __name__ == "__main__":
    main()
