import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { acquireBootCapabilities } from "./lib/runtime";
import { initSession, getDidKey } from "./lib/session";
import { publishOwnBundle } from "./lib/profile";
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

  // Auto-adopt the runtime's user identity. If the runtime has already
  // created a DID for this user (passkey signup in System / hey-home),
  // plant a signed-in profile in localStorage so the app skips the Hey
  // signup page entirely. Two probes, in order of authority:
  //
  //   1. GET /api/session — upstream-canonical "who am I" once the
  //      bearer exchange has resolved. We accept several field names
  //      (did/didKey/principal_id) since the upstream contract is
  //      under active development.
  //   2. .AppData/Identity/profile.json — the cross-capsule shared
  //      identity file. Read via sharedStorage; works on any runtime
  //      that has either patch-0002 or upstream-native /api/localhost
  //      open to third-party capsules.
  //
  // Adoption is READ-only: the user sees the feed under their existing
  // identity. The first time they try a signed action (post, react,
  // comment) and getKeypair() returns null, the existing SignInModal
  // asks for the recovery key (or passkey) once — no new error paths.
  //
  // Idempotent: skipped if a Hey profile is already cached. Silent on
  // failure: any error falls through to the existing Landing page.
  try {
    const hasLocalProfile = !!localStorage.getItem("profile");
    if (hasLocalProfile) {
      console.info("[hey-adopt] local profile already cached — skipping adoption probe");
    } else {
      const adopt = (didKey, name, source, extras = {}) => {
        const adopted = {
          user: {
            id: didKey,
            name: name || "Hey user",
            bio: extras.bio || "",
            avatar: extras.avatar || "",
            didKey,
            role: "general",
            counts: { followers: 0, following: 0 },
          },
          accessToken: "capsule-session",
          refreshToken: "capsule-session",
          accessTokenUpdatedAt: new Date().toISOString(),
          adoptedFromShared: true,
          adoptionSource: source,
        };
        localStorage.setItem("profile", JSON.stringify(adopted));
        console.info(`[hey-adopt] ✓ adopted runtime identity via ${source}`, didKey);
      };

      // Wait for the bearer exchange before any probe — every
      // probe below needs Authorization: Bearer or the runtime's
      // auth_middleware 401s before the handler runs.
      const bearerOk = await bearerReady.catch(() => false);
      console.info(`[hey-adopt] bearer ready = ${bearerOk}`);

      // Probe 1: upstream /api/session.
      let adopted = false;
      try {
        const s = await runtimeSession.current();
        if (!s) {
          console.info("[hey-adopt] probe 1 /api/session → null (401 or empty)");
        } else {
          const did = s?.did || s?.didKey || s?.user?.did || s?.user?.didKey || s?.principal_id;
          if (did) {
            const name = s?.name || s?.user?.name || s?.display_name || s?.user?.display_name;
            adopt(did, name, "api/session", {
              avatar: s?.avatar || s?.user?.avatar,
              bio: s?.bio || s?.user?.bio,
            });
            adopted = true;
          } else {
            console.info("[hey-adopt] probe 1 /api/session → 200 but no DID field. Response:", s);
          }
        }
      } catch (err) {
        console.info("[hey-adopt] probe 1 /api/session threw:", err);
      }

      // Probe 2: shared identity file (canonical + legacy paths).
      if (!adopted) {
        const shared = await readSharedIdentity().catch((err) => {
          console.info("[hey-adopt] probe 2 sharedStorage threw:", err);
          return null;
        });
        if (shared?.didKey) {
          adopt(shared.didKey, shared.name, "shared-identity", {
            avatar: shared.avatar, bio: shared.bio,
          });
          adopted = true;
        } else {
          console.info("[hey-adopt] probe 2 shared-identity file → null (upstream home shell may not write it)");
        }
      }

      // Probe 3 (diagnostic only): upstream's passkey-status. Doesn't
      // give us a DID but tells us whether a principal exists at all,
      // which clarifies whether the empty result above means "no
      // user signed up" (signup is the right next step) or "user
      // exists but we can't see them" (need a different path —
      // e.g., an upstream patch).
      if (!adopted) {
        const ps = await passkeyStatus();
        if (ps) {
          console.info("[hey-adopt] probe 3 /api/auth/passkey/status →", ps);
        } else {
          console.info("[hey-adopt] probe 3 /api/auth/passkey/status → null");
        }
        console.warn("[hey-adopt] no runtime identity found — falling through to Hey signup. If you signed up via passkey in System, check the [hey-adopt] log lines above to see which probe failed.");
      }
    }
  } catch (err) {
    console.warn("[hey-adopt] identity adoption probe failed", err);
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
