import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App.jsx";
import { initSession, getDidKey } from "./lib/session.js";
import { session as runtimeSession, sharedStorage } from "./lib/runtime.js";
import "./index.css";

// Where the messenger UI reads its "I know who you are even if you
// haven't unlocked your signing key yet" identity from. Mirrors the
// Hey Social adoption flow — see capsules/hey-social/client/src/main.jsx.
const ADOPTED_IDENTITY_LS = "hey-messenger-adopted-identity";

// Probe the runtime for an existing user identity. Two sources, in
// order of authority:
//   1. GET /api/session — upstream-canonical "who am I" once the
//      bearer exchange resolved.
//   2. .AppData/Identity/profile.json — cross-capsule shared identity
//      file (same one Hey Social adopts from).
// Caches whatever it finds under ADOPTED_IDENTITY_LS so the UI can
// render "you are X" without a recovery-key entry. Read-only adoption:
// actually sending E2E DMs still requires unlocking the signing key
// (recovery key or passkey) via the messenger's eventual signin flow.
const adoptRuntimeIdentity = async () => {
  if (getDidKey()) return; // Already have a real session — no need to adopt.
  if (localStorage.getItem(ADOPTED_IDENTITY_LS)) return; // Already adopted.

  const remember = (didKey, name, source, extras = {}) => {
    localStorage.setItem(
      ADOPTED_IDENTITY_LS,
      JSON.stringify({
        didKey,
        name: name || "Hey user",
        avatar: extras.avatar || "",
        bio: extras.bio || "",
        source,
        adoptedAt: new Date().toISOString(),
      }),
    );
    console.info(`[hey-messenger] adopted runtime identity (${source})`, didKey);
  };

  try {
    const s = await runtimeSession.current();
    const did = s?.did || s?.didKey || s?.user?.did || s?.user?.didKey || s?.principal_id;
    if (did) {
      const name = s?.name || s?.user?.name || s?.display_name || s?.user?.display_name;
      remember(did, name, "api/session", {
        avatar: s?.avatar || s?.user?.avatar,
        bio: s?.bio || s?.user?.bio,
      });
      return;
    }
  } catch (_) { /* fall through */ }

  try {
    const shared = await sharedStorage.readJson(".AppData/Identity/profile.json");
    if (shared?.didKey) {
      remember(shared.didKey, shared.name, "shared-identity", {
        avatar: shared.avatar, bio: shared.bio,
      });
    }
  } catch (_) { /* silent — no identity to adopt */ }
};

// Populate the in-memory keypair cache (from IDB) before React mounts —
// every signed-event helper assumes session.getKeypair() is non-null for
// signed-in users. Without this, the first render races IDB and signed
// publishes silently fail.
const boot = async () => {
  try { await initSession(); }
  catch (err) { console.warn("[hey-messenger] initSession failed", err); }
  await adoptRuntimeIdentity().catch((err) =>
    console.warn("[hey-messenger] identity adoption probe failed", err),
  );
  createRoot(document.getElementById("root")).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
};
boot();
