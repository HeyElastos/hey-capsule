import { useState, useEffect, useRef } from "react";
import { useStore } from "../state/store.jsx";

// Paste a DID, optionally a friendly name, save. The DID lives on
// disk via the storage adapter; opening this contact creates a DM
// thread routed at the canonical hey-msg/v0/dm/<sortedDids> Carrier
// topic (see lib/inbox.js).
//
// Validation is permissive: anything starting with did:key: is
// accepted at the UI layer. resolveBundle() on the first message
// will surface a more honest "no peer bundle yet" signal.

export default function AddContactModal({ open, onClose }) {
  const { state, addContact } = useStore();
  const [did, setDid] = useState("");
  const [name, setName] = useState("");
  const [err, setErr] = useState(null);
  const didRef = useRef(null);

  // Reset + autofocus on open.
  useEffect(() => {
    if (!open) return;
    setDid(""); setName(""); setErr(null);
    const t = setTimeout(() => didRef.current?.focus(), 60);
    return () => clearTimeout(t);
  }, [open]);

  // Close on Escape; submit on Enter (when DID is non-empty).
  useEffect(() => {
    if (!open) return;
    const onKey = (e) => {
      if (e.key === "Escape") { e.preventDefault(); onClose?.(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  const submit = (e) => {
    e?.preventDefault();
    setErr(null);
    const trimmed = did.trim();
    if (!trimmed) { setErr("Paste a did:key:… string"); return; }
    if (!trimmed.startsWith("did:key:")) {
      setErr("DIDs start with did:key:");
      return;
    }
    if (trimmed === state.currentUser.did) {
      setErr("That's your own DID");
      return;
    }
    try {
      addContact({
        workspaceId: state.activeWorkspaceId,
        did: trimmed,
        name: name.trim(),
      });
      onClose?.();
    } catch (e) {
      setErr(String(e.message || e));
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm p-4"
      onClick={(e) => { if (e.target === e.currentTarget) onClose?.(); }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="add-contact-title"
    >
      <form
        onSubmit={submit}
        className="
          w-full max-w-md
          rounded-2xl
          bg-white/95 dark:bg-zinc-900/95
          backdrop-blur-xl
          border border-zinc-200/70 dark:border-zinc-800/70
          shadow-2xl
          p-6
        "
      >
        <div className="mb-5">
          <h2
            id="add-contact-title"
            className="text-lg font-semibold tracking-tight text-zinc-900 dark:text-zinc-50"
          >
            Add a contact
          </h2>
          <p className="mt-1 text-[13px] text-zinc-500 dark:text-zinc-400">
            Paste your friend's DID (the long <code className="font-mono text-[12px]">did:key:z…</code>
            {" "}string). They share it from their messenger; you can paste yours from below.
          </p>
        </div>

        <label className="block">
          <span className="text-[11px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">DID</span>
          <input
            ref={didRef}
            type="text"
            value={did}
            onChange={(e) => setDid(e.target.value)}
            placeholder="did:key:z6Mk…"
            spellCheck={false}
            className="
              mt-1.5 block w-full font-mono text-[12px]
              rounded-lg px-3 py-2
              bg-zinc-100 dark:bg-zinc-800
              border border-zinc-200 dark:border-zinc-700
              text-zinc-900 dark:text-zinc-100
              placeholder:text-zinc-400 dark:placeholder:text-zinc-500
              outline-none focus:border-amber-400 focus:ring-2 focus:ring-amber-400/30
            "
          />
        </label>

        <label className="mt-4 block">
          <span className="text-[11px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
            Display name <span className="text-zinc-400 dark:text-zinc-500 normal-case font-normal">(optional)</span>
          </span>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Alice"
            maxLength={60}
            className="
              mt-1.5 block w-full text-[14px]
              rounded-lg px-3 py-2
              bg-zinc-100 dark:bg-zinc-800
              border border-zinc-200 dark:border-zinc-700
              text-zinc-900 dark:text-zinc-100
              placeholder:text-zinc-400 dark:placeholder:text-zinc-500
              outline-none focus:border-amber-400 focus:ring-2 focus:ring-amber-400/30
            "
          />
        </label>

        {state.currentUser.did && (
          <div className="mt-5 rounded-lg bg-zinc-100/70 dark:bg-zinc-800/60 px-3 py-2.5">
            <div className="text-[11px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
              Your DID — share this with friends
            </div>
            <div className="mt-1 flex items-center gap-2">
              <code className="font-mono text-[11px] text-zinc-700 dark:text-zinc-300 break-all">
                {state.currentUser.did}
              </code>
              <button
                type="button"
                onClick={() =>
                  navigator.clipboard?.writeText(state.currentUser.did).catch(() => {})
                }
                className="
                  shrink-0 rounded-md px-2 py-0.5 text-[11px] font-medium
                  bg-zinc-200 dark:bg-zinc-700
                  text-zinc-700 dark:text-zinc-300
                  hover:bg-amber-500 hover:text-white
                  transition-colors
                "
              >
                Copy
              </button>
            </div>
          </div>
        )}

        {err && (
          <div className="mt-4 rounded-lg bg-rose-500/10 px-3 py-2 text-[13px] text-rose-600 dark:text-rose-400">
            {err}
          </div>
        )}

        <div className="mt-6 flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={() => onClose?.()}
            className="
              rounded-lg px-3.5 py-1.5 text-[13px] font-medium
              text-zinc-600 dark:text-zinc-400
              hover:bg-zinc-100 dark:hover:bg-zinc-800
              transition-colors
            "
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!did.trim()}
            className="
              rounded-lg px-3.5 py-1.5 text-[13px] font-semibold text-white
              bg-amber-500 hover:bg-amber-600
              disabled:bg-zinc-300 dark:disabled:bg-zinc-700
              disabled:text-zinc-500 dark:disabled:text-zinc-400
              disabled:cursor-not-allowed
              transition-colors
            "
          >
            Add contact
          </button>
        </div>
      </form>
    </div>
  );
}
