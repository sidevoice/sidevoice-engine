// `cargo xtask npm-smoke` (xtask/src/npm.rs) runs this from a directory where @sidevoice/engine was installed from
// its tarball, as a consumer installs it: the package as Node resolves it, its wasm (the file named by the first
// argument, beside the entry point) read from node_modules, and an engine on a plain-object host. It prints what it
// found; the checks are xtask's.
import { readFileSync } from "node:fs";
import { initSync, WebEngine } from "@sidevoice/engine";

const wasm = new URL(process.argv[2], import.meta.resolve("@sidevoice/engine"));
initSync({ module: readFileSync(wasm) });

const host = {
  async capabilities() {
    return { os: "web", arch: "wasm32", accelerators: ["wasm"] };
  },
};
const engine = await WebEngine.create(host);
// No key: listing the providers asks nothing of them.
const providers = (await engine.providers()).map((provider) => provider.id);
console.log(JSON.stringify({ wasm: wasm.pathname, backends: engine.backends(), providers }));
