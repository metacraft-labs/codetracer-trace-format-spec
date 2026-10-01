// Runs a wasm32-wasip1 binary under Node's WASI, preopening the given
// directories at the same paths. Usage:
//   node run_wasi.mjs MODULE.wasm --dir DIR [--dir DIR...] -- ARGS...
import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";
import process from "node:process";

const argv = process.argv.slice(2);
const wasmPath = argv.shift();
const preopens = {};
while (argv[0] === "--dir") {
  argv.shift();
  const d = argv.shift();
  preopens[d] = d;
}
if (argv[0] === "--") argv.shift();
const wasi = new WASI({ version: "preview1", args: [wasmPath, ...argv], env: process.env, preopens });
const mod = await WebAssembly.compile(await readFile(wasmPath));
const inst = await WebAssembly.instantiate(mod, wasi.getImportObject());
process.exitCode = wasi.start(inst);
