import init, {WasmNodeGraph} from "./pkg/cognet_wasm";

async function run() {
  await init();

  const nodeGraph =  new WasmNodeGraph();
  const nodeVariants = nodeGraph.registered_nodes();
  console.log("Variants: ",nodeVariants);
}

run();
