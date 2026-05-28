// Messenger state — workspaces + contacts persist via the runtime's
// storage adapter; messages live in-memory and are re-hydrated from
// Carrier by the inbox poller. currentUser flows from the session (if
// the user unlocked their signing key) or from the boot-time adopted
// identity (read-only fallback).
//
// Loads asynchronously: <App> shows a tiny boot splash until ready=true
// so the rest of the component tree can assume currentUser.did is set.

import {
  createContext,
  useContext,
  useReducer,
  useCallback,
  useEffect,
  useState,
} from "react";
import {
  loadState,
  saveState,
  defaultState,
  addContact as addContactPure,
  removeContact as removeContactPure,
  renameContact as renameContactPure,
} from "../lib/contacts.js";
import { getDidKey } from "../lib/session.js";

const StoreCtx = createContext(null);

const ADOPTED_IDENTITY_LS = "hey-messenger-adopted-identity";

// Read currentUser from the strongest available source. session-derived
// identity (have signing key, can send) wins over the boot-adopted DID
// (read-only). Returns { did, name, canSign } — canSign tells the
// Composer whether to attempt a publish or surface a sign-in prompt.
const readCurrentUser = () => {
  const sessionDid = getDidKey();
  if (sessionDid) {
    let name = sessionDid.slice(0, 14) + "…";
    try {
      const adopted = JSON.parse(localStorage.getItem(ADOPTED_IDENTITY_LS) || "null");
      if (adopted?.didKey === sessionDid && adopted.name) name = adopted.name;
    } catch (_) {}
    return { did: sessionDid, name, canSign: true };
  }
  try {
    const adopted = JSON.parse(localStorage.getItem(ADOPTED_IDENTITY_LS) || "null");
    if (adopted?.didKey) {
      return { did: adopted.didKey, name: adopted.name || "You", canSign: false };
    }
  } catch (_) {}
  return { did: null, name: "", canSign: false };
};

const initialState = (loaded, currentUser) => {
  const firstWs = loaded.workspaces[0];
  const firstContact = (loaded.contactsByWorkspace[firstWs.id] || [])[0];
  return {
    ...loaded,
    messages: {}, // per-thread, in-memory; populated by inbox + sends
    activeWorkspaceId: firstWs.id,
    activeThreadId: firstContact?.id || null,
    inspectorOpen: true,
    currentUser,
    searchQuery: "",
  };
};

const reducer = (state, action) => {
  switch (action.type) {
    case "_replace":
      return action.payload;
    case "set-workspace": {
      const wsId = action.id;
      const firstContact = (state.contactsByWorkspace[wsId] || [])[0];
      return { ...state, activeWorkspaceId: wsId, activeThreadId: firstContact?.id || null };
    }
    case "set-thread":
      return { ...state, activeThreadId: action.id, searchQuery: "" };
    case "toggle-inspector":
      return { ...state, inspectorOpen: !state.inspectorOpen };
    case "set-search":
      return { ...state, searchQuery: action.query || "" };
    case "append-message": {
      const { threadId, message } = action;
      const prior = state.messages[threadId] || [];
      // Dedupe by id — Carrier replays can double-deliver our own sends.
      if (prior.some((m) => m.id === message.id)) return state;
      return {
        ...state,
        messages: { ...state.messages, [threadId]: [...prior, message] },
      };
    }
    case "add-contact": {
      const next = addContactPure(state, action.payload);
      // Auto-select the new contact's thread for instant write.
      const justAdded = (next.contactsByWorkspace[action.payload.workspaceId] || [])
        .find((c) => c.did === action.payload.did);
      return {
        ...next,
        activeWorkspaceId: action.payload.workspaceId,
        activeThreadId: justAdded?.id || state.activeThreadId,
      };
    }
    case "remove-contact":
      return removeContactPure(state, action.payload);
    case "rename-contact":
      return renameContactPure(state, action.payload);
    case "refresh-current-user":
      return { ...state, currentUser: action.user };
    default:
      return state;
  }
};

export const StoreProvider = ({ children }) => {
  const [ready, setReady] = useState(false);
  const [state, dispatch] = useReducer(reducer, null, () =>
    initialState(defaultState(), { did: null, name: "", canSign: false }),
  );

  // Async boot: load persisted state + current user, then unblock render.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const loaded = await loadState();
      const user = readCurrentUser();
      if (cancelled) return;
      dispatch({ type: "_replace", payload: initialState(loaded, user) });
      setReady(true);
    })();
    return () => { cancelled = true; };
  }, []);

  // Persist any change that touches workspaces / contacts. Cheap enough
  // to do on every dispatch since these are small JSON blobs.
  useEffect(() => {
    if (!ready) return;
    saveState(state);
  }, [ready, state.workspaces, state.contactsByWorkspace]);

  const setWorkspace = useCallback((id) => dispatch({ type: "set-workspace", id }), []);
  const setThread = useCallback((id) => dispatch({ type: "set-thread", id }), []);
  const toggleInspector = useCallback(() => dispatch({ type: "toggle-inspector" }), []);
  const setSearch = useCallback((query) => dispatch({ type: "set-search", query }), []);
  const appendMessage = useCallback(
    (threadId, message) => dispatch({ type: "append-message", threadId, message }),
    [],
  );
  const addContact = useCallback(
    (payload) => dispatch({ type: "add-contact", payload }),
    [],
  );
  const removeContact = useCallback(
    (payload) => dispatch({ type: "remove-contact", payload }),
    [],
  );
  const renameContact = useCallback(
    (payload) => dispatch({ type: "rename-contact", payload }),
    [],
  );
  const refreshCurrentUser = useCallback(
    () => dispatch({ type: "refresh-current-user", user: readCurrentUser() }),
    [],
  );

  return (
    <StoreCtx.Provider
      value={{
        state,
        ready,
        setWorkspace,
        setThread,
        toggleInspector,
        setSearch,
        appendMessage,
        addContact,
        removeContact,
        renameContact,
        refreshCurrentUser,
      }}
    >
      {children}
    </StoreCtx.Provider>
  );
};

export const useStore = () => {
  const ctx = useContext(StoreCtx);
  if (!ctx) throw new Error("useStore must be inside <StoreProvider>");
  return ctx;
};
