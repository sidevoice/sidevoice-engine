"""Measures Whisper (STT) and Kokoro (TTS) through mlx-audio, inside the downloaded runtime.

Run by spike.py with the runtime's own interpreter (`python3 -I bench.py ...`), once per device. It prints one JSON
object per line on stdout, each with a "kind"; spike.py collects them. Models are fetched beforehand by `fetch`, so
the timings here never include a network download.

    bench.py fetch   --models DIR --repo ID@REVISION [--repo ...]
    bench.py ready   (imports what a runner would import, then exits: the runner's cold start)
    bench.py licences
    bench.py run     --models DIR --device gpu|cpu --clip WAV --out DIR --whisper NAME [--whisper ...]
"""

import argparse
import json
import statistics
import sys
import time
import wave
from pathlib import Path

T0 = time.perf_counter()

# Kokoro: (case, lang_code, voice, text). The Spanish voice runs through espeak-ng (misaki.espeak).
TTS_CASES = [
    ("en", "a", "af_heart", "The quick brown fox jumps over the lazy dog. Local speech runs on this machine."),
    ("es", "e", "ef_dora", "Hola, esto es una prueba de síntesis de voz en español, hecha en este ordenador."),
]
KOKORO = "Kokoro-82M-bf16"
CALLS = 3


def emit(kind, **fields):
    print(json.dumps({"kind": kind, **fields}, ensure_ascii=False), flush=True)


def timed(fn):
    start = time.perf_counter()
    result = fn()
    return result, time.perf_counter() - start


def fetch(args):
    from huggingface_hub import snapshot_download

    for spec in args.repo:
        repo, revision = spec.split("@")
        target = Path(args.models) / repo.split("/")[-1]
        patterns = ["*.json", "*.safetensors", "*.txt", "*.model", "*.tiktoken"]
        if "Kokoro" in repo:
            patterns = ["config.json", "kokoro-v1_0.safetensors"] + [f"voices/{c[2]}.safetensors" for c in TTS_CASES]
        _, seconds = timed(
            lambda: snapshot_download(repo_id=repo, revision=revision, local_dir=target, allow_patterns=patterns)
        )
        files = [f for f in target.rglob("*") if f.is_file() and ".cache" not in f.parts]
        emit(
            "model_fetch",
            repo=repo,
            revision=revision,
            seconds=round(seconds, 2),
            bytes=sum(f.stat().st_size for f in files),
            files=len(files),
        )


def ready(_args):
    import mlx.core as mx
    import mlx_audio.stt.utils  # noqa: F401
    import mlx_audio.tts.utils  # noqa: F401

    emit("ready", seconds_since_interpreter_start=round(time.perf_counter() - T0, 3), mlx=mx.__version__)


def licences(_args):
    """What each installed distribution declares about its licence, and the licence files it ships."""
    from importlib import metadata

    for dist in sorted(metadata.distributions(), key=lambda d: d.metadata["Name"].lower()):
        meta = dist.metadata
        declared = (meta.get("License") or "").strip().splitlines()
        emit(
            "licence",
            name=meta["Name"],
            version=dist.version,
            expression=meta.get("License-Expression"),
            license_field=declared[0][:120] if declared else None,
            classifiers=[c.split(" :: ")[-1] for c in meta.get_all("Classifier") or [] if c.startswith("License ::")],
            files=[str(f) for f in dist.files or [] if "licen" in str(f).lower() or "copying" in str(f).lower()][:8],
        )


def write_wav(path, samples, rate):
    import numpy as np

    pcm = (np.clip(np.asarray(samples, dtype=np.float32), -1.0, 1.0) * 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.tobytes())


def wav_seconds(path):
    with wave.open(str(path), "rb") as w:
        return w.getnframes() / w.getframerate()


def summarise(times, audio_seconds):
    rest = times[1:] or times
    median = statistics.median(rest)
    return {
        "first_call_s": round(times[0], 3),
        "warm_median_s": round(median, 3),
        "calls": [round(t, 3) for t in times],
        "audio_s": round(audio_seconds, 3),
        "rtf_first": round(times[0] / audio_seconds, 3),
        "rtf_warm": round(median / audio_seconds, 3),
    }


def run(args):
    _, import_s = timed(lambda: __import__("mlx.core"))
    import mlx.core as mx
    import numpy as np

    from mlx_audio.stt.utils import load as load_stt
    from mlx_audio.stt.utils import load_audio
    from mlx_audio.tts.utils import load as load_tts

    device = mx.gpu if args.device == "gpu" else mx.cpu
    mx.set_default_device(device)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    models = Path(args.models)

    info = {}
    try:
        info = {k: (v if isinstance(v, (int, float, str)) else str(v)) for k, v in mx.metal.device_info().items()}
    except Exception as e:  # noqa: BLE001 - recorded, not fatal: this is what we are measuring
        info = {"error": repr(e)}
    emit(
        "device",
        requested=args.device,
        default_device=str(mx.default_device()),
        metal_available=bool(mx.metal.is_available()),
        metal_device_info=info,
        mlx=mx.__version__,
        python=sys.version.split()[0],
        import_mlx_s=round(import_s, 3),
    )

    # A plain matmul, to see whether the GPU is actually faster than the CPU here.
    a = mx.random.normal((2048, 2048))
    mx.eval(a @ a)
    start = time.perf_counter()
    for _ in range(20):
        mx.eval(a @ a)
    seconds = time.perf_counter() - start
    emit(
        "matmul",
        device=args.device,
        n=2048,
        reps=20,
        seconds=round(seconds, 3),
        gflops=round(20 * 2 * 2048**3 / seconds / 1e9, 1),
    )

    # TTS first: its output also feeds STT as a round trip (Kokoro -> Whisper), in both languages.
    tts_model, load_s = timed(lambda: load_tts(str(models / KOKORO)))
    emit("load", device=args.device, model=KOKORO, seconds=round(load_s, 3))
    tts_wavs = []
    for case, lang, voice, text in TTS_CASES:
        voice_path = str(models / KOKORO / "voices" / f"{voice}.safetensors")
        times, audio = [], None
        for _ in range(CALLS):

            def synth():
                chunks = [
                    np.array(r.audio.astype(mx.float32))
                    for r in tts_model.generate(text=text, voice=voice_path, lang_code=lang)
                ]
                return np.concatenate(chunks)

            audio, seconds = timed(synth)
            times.append(seconds)
        rate = tts_model.sample_rate
        wav = out / f"kokoro-{case}-{args.device}.wav"
        write_wav(wav, audio, rate)
        tts_wavs.append((case, wav, text))
        emit(
            "tts",
            device=args.device,
            model=KOKORO,
            case=case,
            voice=voice,
            text=text,
            sample_rate=rate,
            wav=wav.name,
            **summarise(times, len(audio) / rate),
        )
    tts_model = None

    inputs = [("jfk-en", Path(args.clip), "en", None)] + [
        (f"kokoro-{case}", wav, case, text) for case, wav, text in tts_wavs
    ]
    for name in args.whisper:
        stt_model, load_s = timed(lambda: load_stt(str(models / name)))
        emit("load", device=args.device, model=name, seconds=round(load_s, 3))
        # mlx-audio's defaults (temperature fallback 0.0..1.0), then greedy decoding only (temperature 0.0) on the GPU:
        # the fallback samples at higher temperatures when a window fails its compression or log-prob checks.
        variants = [("default", {})] + ([("greedy", {"temperature": 0.0})] if args.device == "gpu" else [])
        for decoding, options in variants:
            for label, path, lang, reference in inputs:
                audio = load_audio(str(path))
                times, result = [], None
                for _ in range(CALLS):
                    result, seconds = timed(lambda: stt_model.generate(audio, language=lang, verbose=False, **options))
                    times.append(seconds)
                emit(
                    "stt",
                    device=args.device,
                    model=name,
                    decoding=decoding,
                    input=label,
                    language=lang,
                    transcript=result.text.strip(),
                    reference=reference,
                    segments=len(result.segments or []),
                    generation_tokens=getattr(result, "generation_tokens", None),
                    **summarise(times, wav_seconds(path)),
                )
        stt_model = None
        mx.clear_cache()

    import resource

    # ru_maxrss is in bytes on macOS. MLX's own peak counts what it allocated, GPU buffers included.
    emit(
        "memory",
        device=args.device,
        peak_rss_bytes=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
        mlx_peak_bytes=mx.get_peak_memory(),
    )


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("fetch")
    p.add_argument("--models", required=True)
    p.add_argument("--repo", action="append", required=True)
    sub.add_parser("ready")
    sub.add_parser("licences")
    p = sub.add_parser("run")
    p.add_argument("--models", required=True)
    p.add_argument("--device", choices=["gpu", "cpu"], required=True)
    p.add_argument("--clip", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--whisper", action="append", required=True)
    args = parser.parse_args()
    {"fetch": fetch, "ready": ready, "licences": licences, "run": run}[args.command](args)


if __name__ == "__main__":
    main()
