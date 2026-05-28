// Persistent contact + workspace state for Hey Messenger.
//
// Stored at `messenger-state.json` via the runtime's storage adapter
// (per-capsule namespace under the user's principal root). Format:
//
//   {
//     v: 1,
//     workspaces: [{ id, name, initials, accent }],
//     contactsByWorkspace: {
//       <workspaceId>: [{ id, did, name, presence }],
//       ...
//     }
//   }
//
// Messages are NOT in this file — they're per-thread, in-memory, and
// re-hydrated from Carrier on demand. This file is small and changes
// rarely (only on add-contact / add-workspace) so a full write on every
// mutation is fine.

import { storage } from "./runtime.js";

const STATE_FILE = "messenger-state.json";
const STATE_VERSION = 1;

const ACCENTS = [
  "from-emerald-500 to-teal-600",
  "from-amber-500 to-orange-600",
  "from-rose-500 to-pink-600",
  "from-sky-500 to-indigo-600",
  "from-violet-500 to-purple-600",
];

// Deterministic accent picker so the same workspace name always gets
// the same color even after a reload.
const accentFor = (seed) => {
  let h = 0;
  for (const c of String(seed || "")) h = (h * 31 + c.charCodeAt(0)) | 0;
  return ACCENTS[Math.abs(h) % ACCENTS.length];
};

// First-launch defaults: one Personal workspace, no contacts. The user
// adds a contact (paste a DID) before they can chat.
export const defaultState = () => {
  const wsId = "ws-personal";
  return {
    v: STATE_VERSION,
    workspaces: [
      { id: wsId, name: "Personal", initials: "P", accent: accentFor("Personal") },
    ],
    contactsByWorkspace: { [wsId]: [] },
  };
};

// Load persisted state. Returns defaultState() on any error or first run.
export const loadState = async () => {
  try {
    const raw = await storage.readJson(STATE_FILE);
    if (!raw || raw.v !== STATE_VERSION) return defaultState();
    // Defensive: validate shape, fall back to defaults on any tear.
    if (!Array.isArray(raw.workspaces) || raw.workspaces.length === 0) return defaultState();
    if (typeof raw.contactsByWorkspace !== "object" || raw.contactsByWorkspace === null) {
      raw.contactsByWorkspace = {};
    }
    return raw;
  } catch (err) {
    console.warn("[hey-messenger] loadState failed; using defaults", err);
    return defaultState();
  }
};

export const saveState = async (state) => {
  const minimal = {
    v: STATE_VERSION,
    workspaces: state.workspaces,
    contactsByWorkspace: state.contactsByWorkspace,
  };
  try { await storage.writeJson(STATE_FILE, minimal); }
  catch (err) { console.warn("[hey-messenger] saveState failed", err); }
};

// Add a contact (DM peer) by DID. Returns the new state. Throws on
// invalid input. Idempotent: adding the same DID to the same workspace
// twice is a no-op (returns the existing record).
export const addContact = (state, { workspaceId, did, name }) => {
  if (!did || typeof did !== "string" || !did.startsWith("did:key:")) {
    throw new Error("Invalid DID — must start with did:key:");
  }
  const trimmedName = (name || "").trim();
  const ws = state.workspaces.find((w) => w.id === workspaceId);
  if (!ws) throw new Error(`Unknown workspace ${workspaceId}`);

  const contacts = state.contactsByWorkspace[workspaceId] || [];
  const existing = contacts.find((c) => c.did === did);
  if (existing) return state;

  const id = `dm-${did.slice(-12)}`;
  const display = trimmedName || `${did.slice(0, 14)}…${did.slice(-6)}`;
  const next = {
    ...state,
    contactsByWorkspace: {
      ...state.contactsByWorkspace,
      [workspaceId]: [
        ...contacts,
        { id, did, name: display, presence: "unknown" },
      ],
    },
  };
  return next;
};

// Remove a contact from a workspace.
export const removeContact = (state, { workspaceId, contactId }) => {
  const contacts = state.contactsByWorkspace[workspaceId] || [];
  return {
    ...state,
    contactsByWorkspace: {
      ...state.contactsByWorkspace,
      [workspaceId]: contacts.filter((c) => c.id !== contactId),
    },
  };
};

// Rename a contact (display name only — DID is immutable).
export const renameContact = (state, { workspaceId, contactId, name }) => {
  const contacts = state.contactsByWorkspace[workspaceId] || [];
  return {
    ...state,
    contactsByWorkspace: {
      ...state.contactsByWorkspace,
      [workspaceId]: contacts.map((c) =>
        c.id === contactId ? { ...c, name: (name || "").trim() || c.name } : c,
      ),
    },
  };
};
