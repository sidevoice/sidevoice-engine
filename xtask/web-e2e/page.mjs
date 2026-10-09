// The web voice loop's page (`cargo xtask web-e2e`, served by run.mjs): the npm package `@sidevoice/engine` as a page
// uses it, through its public interface only. It checks install, cancel, uninstall and the loaded model's
// capabilities, then transcribes the plan's recorded clips and what each text-to-speech model says, and posts what it
// found to /report; xtask judges the transcripts. What it logs is posted to /log, and each model's speech to /speech/.
import init, { WebEngine } from "@sidevoice/engine";

const post = (path, body) => fetch(path, { method: "POST", body });
const log = (...parts) => {
  const line = parts.map((part) => (typeof part === "string" ? part : JSON.stringify(part))).join(" ");
  console.log(line);
  return post("/log", line);
};
// The engine says why something failed in the console (its errors carry only a code): pass it on.
for (const level of ["warn", "error"]) {
  const original = console[level].bind(console);
  console[level] = (...parts) => {
    original(...parts);
    post("/log", `console.${level}: ${parts.map((part) => (part instanceof Error ? part.stack ?? String(part) : typeof part === "string" ? part : JSON.stringify(part) ?? String(part))).join(" ")}`);
  };
}
addEventListener("error", (event) => log("page error:", String(event.message)));
addEventListener("unhandledrejection", (event) => log("unhandled rejection:", String(event.reason?.code ?? event.reason)));

const report = { checks: [], rows: [] };
const check = (name, ok, detail = "") => {
  report.checks.push({ name, ok: Boolean(ok), detail: typeof detail === "string" ? detail : JSON.stringify(detail) });
  return log(ok ? "ok:" : "FAILED:", name, detail);
};
/** The code a promise rejects with, or null if it resolves. */
const rejection = async (promise) => {
  try {
    await promise;
    return null;
  } catch (error) {
    return error?.code ?? String(error);
  }
};
const since = (start) => `${((performance.now() - start) / 1000).toFixed(1)} s`;

/** A WAV file's PCM (16-bit integers), averaged to mono: `{ samples, rate }`. */
function wav(buffer) {
  const view = new DataView(buffer);
  let at = 12;
  let channels = 1;
  let rate = 16_000;
  let bits = 16;
  while (at + 8 <= view.byteLength) {
    const id = String.fromCharCode(...new Uint8Array(buffer, at, 4));
    const size = view.getUint32(at + 4, true);
    if (id === "fmt ") {
      channels = view.getUint16(at + 10, true);
      rate = view.getUint32(at + 12, true);
      bits = view.getUint16(at + 22, true);
    } else if (id === "data") {
      if (bits !== 16) throw new Error(`a ${bits}-bit WAV: only 16-bit PCM is read`);
      const frames = Math.floor(size / 2 / channels);
      const samples = new Float32Array(frames);
      for (let frame = 0; frame < frames; frame++) {
        let sum = 0;
        for (let channel = 0; channel < channels; channel++) {
          sum += view.getInt16(at + 8 + (frame * channels + channel) * 2, true);
        }
        samples[frame] = sum / channels / 32768;
      }
      return { samples, rate };
    }
    at += 8 + size + (size % 2);
  }
  throw new Error("no data chunk");
}

/** `samples` at `rate` as a 16-bit WAV file. */
function toWav(samples, rate) {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  const text = (at, string) => [...string].forEach((c, i) => view.setUint8(at + i, c.charCodeAt(0)));
  text(0, "RIFF");
  view.setUint32(4, 36 + samples.length * 2, true);
  text(8, "WAVEfmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, rate, true);
  view.setUint32(28, rate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, "data");
  view.setUint32(40, samples.length * 2, true);
  samples.forEach((sample, i) => view.setInt16(44 + i * 2, Math.max(-1, Math.min(1, sample)) * 32767, true));
  return buffer;
}

try {
  await init();
  const plan = await (await fetch("/plan.json")).json();
  const engine = await WebEngine.create({
    async capabilities() {
      return { os: "web", arch: "wasm32", accelerators: plan.accelerators, cores: navigator.hardwareConcurrency ?? null };
    },
  });
  const model = async (id) => (await engine.models()).find((model) => model.id === id);

  for (const wanted of [plan.stt, ...plan.tts]) {
    const build = (await model(wanted.model))?.builds.find((build) => build.id === wanted.build);
    await check(`${wanted.build} runs here`, build?.available, build ? build.reasons : "no such build");
  }

  // A previous run in the same profile would have left it installed: start from nothing.
  await engine.uninstall(plan.stt.model);
  const controller = new AbortController();
  const aborted = await rejection(
    engine.install(plan.stt.model, plan.stt.build, (progress) => progress.received > 0 && controller.abort(), controller.signal),
  );
  await check("an install aborted part way rejects with cancelled", aborted === "cancelled", aborted);
  await check("an aborted install leaves the model uninstalled", !(await model(plan.stt.model)).installed);

  let start = performance.now();
  let last = null;
  await engine.install(plan.stt.model, plan.stt.build, (progress) => (last = progress));
  await log(`installed ${plan.stt.build} in ${since(start)}`);
  await check("an install reports its progress to the last file", last && last.done === last.files, last);
  await check("an installed model says so", (await model(plan.stt.model)).installed);

  start = performance.now();
  const whisper = await engine.load(plan.stt.model, plan.stt.build);
  await log(`loaded ${whisper.build} in ${since(start)}`);
  await check(
    "Whisper loads as speech to text only",
    whisper.capabilities().join() === "stt" && whisper.asTts() === undefined,
    whisper.capabilities(),
  );
  const stt = whisper.asStt();
  const inUse = await rejection(engine.uninstall(plan.stt.model));
  await check("a loaded model cannot be uninstalled", inUse === "model-in-use", inUse);

  for (const clip of plan.clips) {
    const row = {
      pair: `${plan.stt.model} ← clip ${clip.name}`,
      language: clip.language,
      said: clip.text,
    };
    try {
      const { samples, rate } = wav(await (await fetch(clip.file)).arrayBuffer());
      start = performance.now();
      row.heard = await stt.transcribe(samples, rate, clip.language);
      await log(`${row.pair}: ${since(start)}: ${row.heard}`);
    } catch (error) {
      row.error = error?.code ?? String(error);
      await log(`${row.pair}: failed:`, row.error, String(error?.stack ?? ""));
    }
    report.rows.push(row);
  }

  for (const speaker of plan.tts) {
    const row = {
      pair: `${plan.stt.model} ← ${speaker.model} (${speaker.voice})`,
      language: speaker.language,
      said: speaker.sentence,
    };
    try {
      start = performance.now();
      const loaded = await engine.load(speaker.model, speaker.build, (progress) => (last = progress));
      await log(`installed and loaded ${loaded.build} in ${since(start)}`);
      const tts = loaded.asTts();
      const voices = await tts.voices();
      await check(`${speaker.model} has the voice ${speaker.voice}`, voices.some((voice) => voice.id === speaker.voice), voices.map((voice) => voice.id));
      start = performance.now();
      const audio = await tts.speak(speaker.sentence, speaker.voice, speaker.language);
      const seconds = audio.samples.length / audio.sampleRate;
      await log(`${speaker.model} spoke ${seconds.toFixed(1)} s at ${audio.sampleRate} Hz in ${since(start)}`);
      await post(`/speech/${speaker.model}-${speaker.voice}.wav`, toWav(audio.samples, audio.sampleRate));
      row.heard = await stt.transcribe(audio.samples, audio.sampleRate, speaker.language);
      await log(`${row.pair}: ${row.heard}`);
      tts.free();
      loaded.free();
    } catch (error) {
      row.error = error?.code ?? String(error);
      await log(`${row.pair}: failed:`, row.error, String(error?.stack ?? ""));
    }
    report.rows.push(row);
  }

  stt.free();
  whisper.free();
  const uninstalled = await rejection(engine.uninstall(plan.stt.model));
  await check("once freed, a model uninstalls", uninstalled === null, uninstalled);
  await check("an uninstalled model says so", !(await model(plan.stt.model)).installed);
} catch (error) {
  report.error = { code: error?.code ?? null, message: String(error?.message ?? error), stack: String(error?.stack ?? "") };
  await log("error:", report.error);
}
await post("/report", JSON.stringify(report));
