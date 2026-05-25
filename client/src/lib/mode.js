// Capsule vs. server mode detector.
//
// Hey supports two deployment shapes:
//   - "server" — the classic deploy (YunoHost, dev with Express on :4000).
//                Hey's React app talks to Hey's Node backend at /api/...
//   - "capsule" — Hey is a WASM capsule on the Elastos Runtime. There is no
//                 Hey backend; React talks to the runtime's APIs:
//                 /api/provider/<scheme>/<op>, /api/localhost/<path>, etc.
//
// Build-time decision via the VITE_HEY_MODE env var. Defaults to "server" so
// existing builds are unaffected.
//
// Run:
//   VITE_HEY_MODE=server  npm run build   (default)
//   VITE_HEY_MODE=capsule vite build --base ./   (for the capsule bundle)

const MODE =
  (typeof import.meta !== "undefined" && import.meta.env?.VITE_HEY_MODE) ||
  "server";

export const isCapsuleMode = () => MODE === "capsule";
export const isServerMode = () => MODE === "server";
export const mode = () => MODE;
