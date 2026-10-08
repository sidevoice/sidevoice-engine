# Spike: MLX through a Python mlx-audio runner (#36)

Measurements only, not engine code and not for merge. `spike.py` installs a standalone Python and the pinned wheels of
`requirements.txt` into a directory of its own, as an app would download a runtime on demand, then measures size,
cold start, model load and time per call for Whisper and Kokoro (`bench.py`, run with that Python) on the GPU and on
the CPU, and how the result is signed. CI runs it on `macos-14` (`.github/workflows/spike-mlx-python.yml`); on an
Apple-silicon Mac:

    python3 spikes/mlx-python/spike.py --out /tmp/spike

It downloads about 3 GB of models. `requirements.in` says how `requirements.txt` is regenerated.
