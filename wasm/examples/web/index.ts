import init, {WasmNodeGraph} from "./pkg/cognet_wasm";

async function run() {
  await init();

  const nodeGraph =  new WasmNodeGraph();
  nodeGraph.execute();
}

run();
