import { useEffect, useState } from "react";
import { StoreProvider, useStore } from "./state/store.jsx";
import WorkspaceRail from "./components/WorkspaceRail.jsx";
import ChannelList from "./components/ChannelList.jsx";
import Conversation from "./components/Conversation.jsx";
import Composer from "./components/Composer.jsx";
import Inspector from "./components/Inspector.jsx";
import EncryptionBadge from "./components/EncryptionBadge.jsx";
import AddContactModal from "./components/AddContactModal.jsx";
import { startInbox, dmTopic } from "./lib/inbox.js";
import { publishOwnBundle } from "./lib/profile.js";
import { getDidKey } from "./lib/session.js";

const ChannelHeader = ({ onAddContact }) => {
  const { state, toggleInspector, setSearch } = useStore();
  const contacts = state.contactsByWorkspace[state.activeWorkspaceId] || [];
  const d = contacts.find((x) => x.id === state.activeThreadId);
  const name = d ? d.name : "—";
  const subtitle = d ? "direct message" : "no thread selected";
  // E2E only honest when (a) it's a DM (b) we have the peer's DID.
  // Every contact has a DID now (no mock fallback), so DM → e2e.
  const encKind = d ? "e2e" : "transit";
  return (
    <header
      className="
        flex items-center gap-3
        px-5 py-3
        bg-white/40 dark:bg-zinc-900/30
        backdrop-blur-xl
        border-b border-zinc-200/60 dark:border-zinc-800/60
      "
    >
      <div className="flex items-center gap-3 min-w-0">
        <div className="min-w-0">
          <div className="text-base font-semibold tracking-tight truncate">{name}</div>
          <div className="text-[11px] text-zinc-500 dark:text-zinc-400">{subtitle}</div>
        </div>
        {d && <EncryptionBadge kind={encKind} />}
      </div>

      <div className="flex-1 max-w-sm ml-auto">
        <div className="relative">
          <span className="absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400 dark:text-zinc-500 pointer-events-none">
            <SearchIcon />
          </span>
          <input
            type="search"
            value={state.searchQuery}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={d ? `Search in ${d.name}` : "Search"}
            disabled={!d}
            className="
              w-full rounded-lg pl-9 pr-3 py-1.5 text-[13px]
              bg-zinc-100/70 dark:bg-zinc-800/60
              border border-zinc-200/60 dark:border-zinc-700/60
              text-zinc-900 dark:text-zinc-100
              placeholder:text-zinc-400 dark:placeholder:text-zinc-500
              outline-none focus:border-amber-400 focus:ring-2 focus:ring-amber-400/30
              disabled:opacity-50 disabled:cursor-not-allowed
              transition
            "
          />
        </div>
      </div>

      <div className="flex items-center gap-1">
        <IconBtn title="Add contact" onClick={onAddContact}>
          <PlusIcon />
        </IconBtn>
        <button
          disabled
          title="Video calls — coming soon (P2P over Carrier-signaled WebRTC)"
          aria-label="Video calls — coming soon"
          className="
            relative rounded-lg p-1.5 text-zinc-400 dark:text-zinc-500
            cursor-not-allowed opacity-60
          "
        >
          <VideoIcon />
          <span className="absolute -top-1 -right-1 rounded-full bg-amber-500/90 text-[8px] font-bold uppercase tracking-wider text-white px-1 py-[1px] leading-none">
            soon
          </span>
        </button>
        <IconBtn title="Toggle inspector" onClick={toggleInspector}>
          <PanelIcon />
        </IconBtn>
      </div>
    </header>
  );
};

const IconBtn = ({ children, title, onClick }) => (
  <button
    title={title}
    onClick={onClick}
    className="rounded-lg p-1.5 text-zinc-500 hover:bg-amber-500/10 hover:text-amber-600 dark:text-zinc-400 dark:hover:text-amber-400 transition-colors"
  >
    {children}
  </button>
);

const Backdrop = ({ children }) => (
  <div
    className="
      relative h-full w-full overflow-hidden
      bg-gradient-to-br
      from-amber-50 via-rose-50 to-zinc-100
      dark:from-zinc-950 dark:via-zinc-950 dark:to-zinc-900
    "
  >
    <div aria-hidden className="pointer-events-none absolute -top-32 -left-32 h-96 w-96 rounded-full bg-amber-400/20 blur-3xl dark:bg-amber-500/10" />
    <div aria-hidden className="pointer-events-none absolute -bottom-32 -right-32 h-96 w-96 rounded-full bg-rose-400/20 blur-3xl dark:bg-rose-500/10" />
    {children}
  </div>
);

// Build the live list of Carrier topics for the active workspace.
// Each contact's DM topic is canonical between the two DIDs (sorted),
// so both peers' messengers agree on the topic string.
const buildTopicList = (state) => {
  const myDid = state.currentUser.did;
  if (!myDid) return [];
  const contacts = state.contactsByWorkspace[state.activeWorkspaceId] || [];
  return contacts.map((c) => ({
    id: c.id,
    topic: dmTopic(myDid, c.did),
    kind: "dm",
  }));
};

const Shell = () => {
  const { state, ready, appendMessage, addContact } = useStore();
  const [addOpen, setAddOpen] = useState(false);

  // Boot the inbox poller + publish our profile bundle once. Re-runs
  // when the workspace or current user changes so the topic list
  // matches what's on screen.
  useEffect(() => {
    if (!ready) return;
    if (!getDidKey()) return; // adoption-only mode: read-only, no Carrier wiring yet
    publishOwnBundle().catch((err) => {
      console.warn("[hey-messenger] profile bundle publish failed", err);
    });
    const stop = startInbox({
      topics: () => buildTopicList(state),
      onMessage: ({ threadId, message }) => {
        if (message.sender_did === state.currentUser.did) return;
        const payload = message.payload_decrypted ?? message.payload ?? {};
        appendMessage(threadId, {
          id: message.id || `remote-${message.ts}-${message.sender_did?.slice(-6)}`,
          sender_did: message.sender_did,
          sender_name: message.sender_name || message.sender_did?.slice(0, 14) + "…",
          ts: message.ts,
          payload: message.encryptedButUnreadable
            ? { content: "🔒 encrypted message — no key", _unreadable: true }
            : payload,
        });
      },
    });
    return () => stop();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, state.activeWorkspaceId, state.currentUser.did]);

  if (!ready) {
    return (
      <Backdrop>
        <div className="flex h-full items-center justify-center text-sm text-zinc-500 dark:text-zinc-400">
          Loading…
        </div>
      </Backdrop>
    );
  }

  return (
    <Backdrop>
      <div className="relative z-10 flex h-full">
        <WorkspaceRail />
        <ChannelList onAddContact={() => setAddOpen(true)} />
        <main className="flex flex-1 flex-col min-w-0">
          <ChannelHeader onAddContact={() => setAddOpen(true)} />
          {state.activeThreadId ? (
            <>
              <Conversation />
              <Composer />
            </>
          ) : (
            <EmptyState onAddContact={() => setAddOpen(true)} canSign={state.currentUser.canSign} />
          )}
        </main>
        {state.inspectorOpen && <Inspector />}
      </div>
      <AddContactModal open={addOpen} onClose={() => setAddOpen(false)} />
    </Backdrop>
  );
};

const EmptyState = ({ onAddContact, canSign }) => (
  <div className="flex flex-1 items-center justify-center px-6">
    <div className="max-w-md text-center">
      <div className="text-5xl mb-3">💬</div>
      <h2 className="text-xl font-semibold tracking-tight text-zinc-900 dark:text-zinc-50">
        No conversations yet
      </h2>
      <p className="mt-2 text-[14px] text-zinc-500 dark:text-zinc-400">
        Add a friend by their DID to start a peer-to-peer, end-to-end-encrypted chat.
        File transfers go direct via iroh-blobs — no size limit, no server in the middle.
      </p>
      <button
        type="button"
        onClick={onAddContact}
        className="
          mt-5 inline-flex items-center rounded-lg
          bg-amber-500 px-4 py-2 text-[14px] font-semibold text-white
          hover:bg-amber-600 transition-colors
        "
      >
        Add a contact
      </button>
      {!canSign && (
        <p className="mt-4 text-[12px] text-zinc-500 dark:text-zinc-500">
          Reading as your runtime identity. Sending requires unlocking your signing key.
        </p>
      )}
    </div>
  </div>
);

export default function App() {
  return (
    <StoreProvider>
      <Shell />
    </StoreProvider>
  );
}

// — icons —
const VideoIcon = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <polygon points="23 7 16 12 23 17 23 7" />
    <rect x="1" y="5" width="15" height="14" rx="2" ry="2" />
  </svg>
);
const SearchIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="11" cy="11" r="7" />
    <line x1="21" y1="21" x2="16.65" y2="16.65" />
  </svg>
);
const PanelIcon = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="3" width="18" height="18" rx="2" />
    <line x1="15" y1="3" x2="15" y2="21" />
  </svg>
);
const PlusIcon = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <line x1="12" y1="5" x2="12" y2="19" />
    <line x1="5" y1="12" x2="19" y2="12" />
  </svg>
);
