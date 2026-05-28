import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { acquireBootCapabilities } from "./lib/runtime";
import { initSession, getDidKey } from "./lib/session";
import { publishOwnBundle } from "./lib/profile";
import { readSharedIdentity } from "./lib/shell";
import "./index.css";

// Derive the router basename from the iframe's mount path. Under YunoHost
// the capsule loads at /elastos/apps/hey-social/, not at /. Without this
// react-router would try to match the full pathname against the app's
// routes (/, /videos, /profile, etc.), fail every match, and render
// nothing — the blank-white-window symptom.
const ROUTER_BASENAME = (() => {
  if (typeof window === "undefined") return "/";
  const m = window.location.pathname.match(/^(.*?\/apps\/[^/]+)\//);
  return m ? m[1] : "/";
})();

// Hardened-key session load must complete BEFORE React mounts:
// getKeypair()/getDidKey() return null until the IDB CryptoKey is in
// the cache. Mounting first would briefly render the signed-out view
// for a signed-in user. initSession() failures fall through to the
// signed-out view, which is the correct safe default.
const boot = async () => {
  // Capability acquisition is non-blocking — runs in parallel with
  // the session init. Tokens land in sessionStorage by the time the
  // first user-driven fetch happens.
  acquireBootCapabilities().catch(() => { /* logged inside helper */ });

  try {
    await initSession();
  } catch (err) {
    console.warn("[hey] initSession failed; rendering as signed-out", err);
  }

  // Auto-adopt the runtime's user identity. If the runtime (or another
  // capsule on this node) has already created a DID for this user, plant
  // a signed-in profile in localStorage so the app skips the Hey signup
  // page entirely. Read-only adoption: the user sees the feed under
  // their existing identity; if they attempt a signed action without a
  // local signing key in IDB, the existing SignInModal asks for the
  // recovery key (or passkey) one time. Idempotent — skips if a Hey
  // profile is already cached.
  try {
    const hasLocalProfile = !!localStorage.getItem("profile");
    if (!hasLocalProfile) {
      const shared = await readSharedIdentity().catch(() => null);
      if (shared?.didKey) {
        const adopted = {
          user: {
            id: shared.didKey,
            name: shared.name || "Hey user",
            bio: shared.bio || "",
            avatar: shared.avatar || "",
            didKey: shared.didKey,
            role: "general",
            counts: { followers: 0, following: 0 },
          },
          accessToken: "capsule-session",
          refreshToken: "capsule-session",
          accessTokenUpdatedAt: new Date().toISOString(),
          adoptedFromShared: true,
        };
        localStorage.setItem("profile", JSON.stringify(adopted));
        console.info("[hey] adopted runtime identity", shared.didKey);
      }
    }
  } catch (err) {
    console.warn("[hey] shared-identity adoption probe failed", err);
  }

  // Publish our hybrid-PQ pubkey bundle so peers can E2E-encrypt DMs to
  // us. Non-blocking — first peer to want to DM us subscribes to our
  // profile topic and pulls the latest. Sessions that predate the PQ
  // upgrade have no x25519/kem keys; publishOwnBundle returns null
  // silently in that case and falls back to transit-only.
  if (getDidKey()) {
    publishOwnBundle().catch((err) => {
      console.warn("[hey] profile bundle publish failed", err);
    });
  }

  ReactDOM.createRoot(document.getElementById("root")).render(
    <BrowserRouter
      basename={ROUTER_BASENAME}
      future={{ v7_startTransition: true, v7_relativeSplatPath: true }}
    >
      <App />
    </BrowserRouter>
  );
};

boot();
