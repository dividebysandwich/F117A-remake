// Trunk's initializer (the data-initializer in index.html): the loading screen's progress bar.
// Trunk counts unpacked bytes against the wasm's size, so the share is right whatever the server
// compresses. The megabytes shown are that share of the gzipped size (version.js), what a server
// with gzip_static sends.
const $ = id => document.getElementById(id);
const mb = bytes => (bytes / 1048576).toFixed(1);
const wire = (bytes, total) => (window.SM89_WASM_GZ ? (bytes / total) * window.SM89_WASM_GZ : bytes);

export default function loader() {
  return {
    onStart() {
      $("phase").textContent = "DOWNLOADING";
    },
    onProgress({ current, total }) {
      const pct = Math.min(100, Math.floor((current / total) * 100));
      $("fill").style.width = `${pct}%`;
      $("bar").setAttribute("aria-valuenow", pct);
      $("pct").textContent = `${pct}%`;
      $("bytes").textContent = `${mb(wire(current, total))} / ${mb(wire(total, total))} MB`;
      // Downloaded: the browser compiles the wasm, which takes a while.
      if (current >= total) {
        $("phase").textContent = "STARTING";
        $("bar").classList.add("busy");
      }
    },
    onComplete() {},
    onSuccess() {},
    onFailure(error) {
      // Trunk still sends TrunkApplicationStarted: index.html keeps this screen up.
      $("status").dataset.failed = "";
      $("phase").textContent = "LOADING FAILED";
      $("bar").classList.remove("busy");
      $("bar").classList.add("failed");
      $("bytes").textContent = String(error);
    },
  };
}
