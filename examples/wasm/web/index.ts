import init, { add } from "./pkg/wasm";

async function run() {
  await init();
  console.log(add(3, 4));
}

run();
