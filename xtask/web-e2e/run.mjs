// `cargo xtask web-e2e` (xtask/src/web_e2e.rs) runs this in Node, with no dependency of its own: it serves SITE (the
// page, its plan and clips, and the npm package installed in SITE/node_modules as a consumer installs it) on a port of
// 127.0.0.1, with an import map that resolves the packages' browser entry points, opens it in a headless CHROME, and
// waits for the page to post its report, which it prints on standard output as JSON. What the page logs goes to
// standard error; the speech it posts is written to SPEECH, as WAV files to listen to.
//
// usage: node run.mjs SITE CHROME SPEECH [TIMEOUT_MINUTES]
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, normalize, sep } from "node:path";

const [site, chrome, speech, minutes = "30"] = process.argv.slice(2);
if (!site || !chrome || !speech) {
  console.error("usage: node run.mjs SITE CHROME SPEECH [TIMEOUT_MINUTES]");
  process.exit(2);
}

// The packages the page imports by name, directly or through another package.
const SPECIFIERS = [
  "@sidevoice/engine",
  "@huggingface/transformers",
  "onnxruntime-web",
  "onnxruntime-web/webgpu",
  "onnxruntime-common",
  "espeak-ng",
];
// A browser's conditions, as a bundler targeting one would resolve `exports`.
const CONDITIONS = new Set(["browser", "import", "module", "default"]);

/** The entry point `target` (a path, or conditions) names for a browser, if any. */
function pick(target) {
  if (typeof target === "string") return target;
  if (!target || typeof target !== "object") return null;
  for (const [key, value] of Object.entries(target)) {
    if (CONDITIONS.has(key)) {
      const picked = pick(value);
      if (picked) return picked;
    }
  }
  return null;
}

/** Where `specifier` resolves in the browser, as a URL path under /node_modules, or null. */
function resolve(specifier) {
  const parts = specifier.split("/");
  const name = specifier.startsWith("@") ? parts.slice(0, 2).join("/") : parts[0];
  const sub = "." + specifier.slice(name.length);
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(join(site, "node_modules", name, "package.json"), "utf8"));
  } catch {
    return null;
  }
  let entry;
  if (manifest.exports) {
    const exports = manifest.exports;
    const keyed = typeof exports === "object" && Object.keys(exports).some((key) => key.startsWith("."));
    entry = pick(keyed ? exports[sub] : sub === "." ? exports : null);
  } else if (sub === ".") {
    entry = manifest.browser && typeof manifest.browser === "string" ? manifest.browser : manifest.module ?? manifest.main ?? "index.js";
  }
  return entry ? `/node_modules/${name}/${entry.replace(/^\.\//, "")}` : null;
}

const imports = {};
for (const specifier of SPECIFIERS) {
  const resolved = resolve(specifier);
  if (resolved) imports[specifier] = resolved;
}
console.error(`import map: ${JSON.stringify(imports)}`);
const index = `<!doctype html>
<meta charset="utf-8">
<title>sidevoice-engine web e2e</title>
<script type="importmap">${JSON.stringify({ imports })}</script>
<script type="module" src="/page.mjs"></script>
`;

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".wav": "audio/wav",
};

function body(request) {
  return new Promise((done, failed) => {
    const parts = [];
    request.on("data", (part) => parts.push(part));
    request.on("end", () => done(Buffer.concat(parts)));
    request.on("error", failed);
  });
}

let finish;
const finished = new Promise((done) => (finish = done));

const server = createServer(async (request, response) => {
  const url = new URL(request.url, "http://localhost");
  try {
    if (request.method === "POST") {
      const bytes = await body(request);
      if (url.pathname === "/log") process.stderr.write(`page: ${bytes.toString("utf8")}\n`);
      else if (url.pathname.startsWith("/speech/")) {
        const name = url.pathname.slice("/speech/".length).replace(/[^\w.-]/g, "_");
        writeFileSync(join(speech, name), bytes);
      } else if (url.pathname === "/report") finish(bytes.toString("utf8"));
      response.writeHead(204).end();
      return;
    }
    if (url.pathname === "/" || url.pathname === "/index.html") {
      response.writeHead(200, { "content-type": TYPES[".html"] }).end(index);
      return;
    }
    const path = normalize(join(site, decodeURIComponent(url.pathname)));
    if (!path.startsWith(normalize(site) + sep) || !statSync(path, { throwIfNoEntry: false })?.isFile()) {
      response.writeHead(404).end();
      return;
    }
    const type = TYPES[extname(path)] ?? "application/octet-stream";
    response.writeHead(200, { "content-type": type }).end(readFileSync(path));
  } catch (error) {
    process.stderr.write(`server: ${request.method} ${url.pathname}: ${error}\n`);
    response.writeHead(500).end();
  }
});

await new Promise((done) => server.listen(0, "127.0.0.1", done));
const page = `http://127.0.0.1:${server.address().port}/`;
const profile = mkdtempSync(join(tmpdir(), "sidevoice-web-e2e-"));
const browser = spawn(
  chrome,
  ["--headless=new", "--no-sandbox", "--no-first-run", "--no-default-browser-check", `--user-data-dir=${profile}`, page],
  { stdio: ["ignore", "ignore", "pipe"] },
);
browser.stderr.on("data", () => {}); // Chrome's own chatter: not ours.
browser.on("exit", (code) => finish(JSON.stringify({ error: { message: `Chrome exited (${code}) before the page reported` } })));
const timeout = setTimeout(
  () => finish(JSON.stringify({ error: { message: `no report after ${minutes} minutes` } })),
  Number(minutes) * 60_000,
);

const report = await finished;
clearTimeout(timeout);
browser.removeAllListeners("exit");
browser.kill();
server.close();
server.closeAllConnections();
try {
  rmSync(profile, { recursive: true, force: true });
} catch {
  // Chrome may still be writing to it as it exits: a temporary directory, left behind.
}
process.stdout.write(report + "\n");
