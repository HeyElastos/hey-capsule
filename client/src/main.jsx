import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { isCapsuleMode } from "./lib/mode";
import { acquireBootCapabilities } from "./lib/runtime";
import { initSession } from "./lib/session";
import "./index.css";

// Hardened-key session load must complete BEFORE React mounts:
// the sync getKeypair()/getDidKey() getters return null until the
// IDB CryptoKey is in the cache. Mounting first would briefly render
// the signed-out view for a signed-in user. We await initSession on
// the boot path; failures are logged and the app falls through to
// the signed-out view, which is the correct safe default.
const boot = async () => {
  // Capability acquisition is non-blocking — runs in parallel with
  // the session init. Tokens land in sessionStorage by the time the
  // first user-driven fetch happens.
  if (isCapsuleMode()) {
    acquireBootCapabilities().catch(() => { /* logged inside helper */ });
  }

  try {
    await initSession();
  } catch (err) {
    console.warn("[hey] initSession failed; rendering as signed-out", err);
  }

  ReactDOM.createRoot(document.getElementById("root")).render(
    <BrowserRouter
      future={{ v7_startTransition: true, v7_relativeSplatPath: true }}
    >
      <App />
    </BrowserRouter>
  );
};

boot();
