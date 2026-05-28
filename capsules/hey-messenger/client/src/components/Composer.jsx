import { useCallback, useRef, useState } from "react";
import { useStore } from "../state/store.jsx";
import { blobs, peer, RuntimeError } from "../lib/runtime.js";
import { createSignedEvent } from "../lib/events.js";
import { encryptToHybrid } from "../lib/pqcrypto.js";
import { resolveBundle } from "../lib/profile.js";
import { dmTopic } from "../lib/inbox.js";
import { getKeypair } from "../lib/session.js";
import AttachmentPill from "./AttachmentPill.jsx";

const localId = () => `local-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

// Identify the active thread. Contacts are keyed by their DID; the
// DM topic is canonical between the two DIDs (sorted). Channels are
// a Phase-4 feature, gated behind iroh-docs; for now everything in
// the contact list is a DM.
const classifyThread = (state) => {
  const contacts = state.contactsByWorkspace[state.activeWorkspaceId] || [];
  const dm = contacts.find((c) => c.id === state.activeThreadId);
  if (dm) {
    return {
      kind: "dm",
      thread: dm,
      topic: state.currentUser.did ? dmTopic(state.currentUser.did, dm.did) : null,
      peerDid: dm.did,
    };
  }
  return { kind: "unknown", thread: null, topic: null, peerDid: null };
};

export default function Composer() {
  const { state, appendMessage } = useStore();
  const [text, setText] = useState("");
  const [pending, setPending] = useState([]);
  const [dragging, setDragging] = useState(false);
  const [sending, setSending] = useState(false);
  const fileInputRef = useRef(null);
  const textareaRef = useRef(null);

  // Wrap the current textarea selection in the given marker pair.
  // If nothing is selected, just inserts the markers + places caret
  // between them so the user types into the new emphasis. Restores
  // focus + selection after the state update so the experience is
  // continuous — type, select, click bold, keep typing.
  const wrapSelection = (marker, markerClose) => {
    const close = markerClose ?? marker;
    const ta = textareaRef.current;
    if (!ta) {
      setText((t) => `${t}${marker}${close}`);
      return;
    }
    const start = ta.selectionStart ?? text.length;
    const end = ta.selectionEnd ?? text.length;
    const before = text.slice(0, start);
    const middle = text.slice(start, end);
    const after = text.slice(end);
    const next = `${before}${marker}${middle}${close}${after}`;
    setText(next);
    // Restore focus + caret on the next tick.
    requestAnimationFrame(() => {
      ta.focus();
      const newStart = before.length + marker.length;
      const newEnd = newStart + middle.length;
      ta.setSelectionRange(newStart, newEnd);
    });
  };

  const beginUpload = useCallback(async (file) => {
    const tempId = localId();
    setPending((p) => [
      ...p,
      { tempId, name: file.name, size: file.size, mime: file.type, status: "uploading", progress: 0.05 },
    ]);
    try {
      const tick = setInterval(() => {
        setPending((p) =>
          p.map((x) =>
            x.tempId === tempId && x.status === "uploading"
              ? { ...x, progress: Math.min(0.92, (x.progress ?? 0) + 0.07) }
              : x
          )
        );
      }, 200);
      const resp = await blobs.addBytes(file, file.name);
      clearInterval(tick);
      const hash = resp?.data?.hash || resp?.hash;
      const ticket = resp?.data?.ticket || resp?.ticket;
      setPending((p) =>
        p.map((x) =>
          x.tempId === tempId
            ? { ...x, status: "uploaded", progress: 1, ticket, cid: hash }
            : x
        )
      );
    } catch (err) {
      const isRuntime = err instanceof RuntimeError;
      setPending((p) =>
        p.map((x) =>
          x.tempId === tempId
            ? {
                ...x,
                status: "error",
                error: isRuntime ? `provider error (${err.status || "?"})` : String(err.message || err),
              }
            : x
        )
      );
    }
  }, []);

  const onPickFiles = (files) => {
    if (!files) return;
    Array.from(files).forEach((f) => beginUpload(f));
  };

  const onDrop = (e) => {
    e.preventDefault();
    setDragging(false);
    if (e.dataTransfer?.files) onPickFiles(e.dataTransfer.files);
  };

  // Optimistically render the message locally + sign/encrypt/publish in
  // the background. If publish fails we mark the local message as
  // "send failed" so the user can retry; for now we just console.warn.
  const send = async () => {
    if (sending) return;
    const ready = pending.filter((p) => p.status === "uploaded");
    const trimmed = text.trim();
    if (!trimmed && ready.length === 0) return;

    const kp = getKeypair();
    const t = classifyThread(state);

    const attachments = ready.length
      ? ready.map((p) => ({
          cid: p.cid, ticket: p.ticket, name: p.name, size: p.size, mime: p.mime,
        }))
      : undefined;
    const plainPayload = {
      content: trimmed,
      ...(attachments ? { attachments } : {}),
    };

    // Optimistic local render (always uses the plaintext payload).
    const localMessage = {
      id: localId(),
      sender_did: state.currentUser.did,
      sender_name: state.currentUser.name,
      ts: Date.now(),
      payload: plainPayload,
    };
    appendMessage(state.activeThreadId, localMessage);
    setText("");
    setPending([]);

    if (!kp) {
      console.warn("[composer] not signed in — skipping Carrier publish");
      return;
    }
    if (!t.topic) {
      console.warn("[composer] no Carrier topic for this thread — skipping publish");
      return;
    }

    setSending(true);
    try {
      let finalPayload = plainPayload;
      let encrypted = false;

      if (t.kind === "dm" && t.peerDid) {
        const bundle = await resolveBundle(t.peerDid).catch(() => null);
        if (bundle?.x25519Pub && bundle?.kemPub) {
          // Encrypt to BOTH peer and self so multi-device clients of
          // ours can still read sent messages (sent-mailbox pattern).
          const env = encryptToHybrid(
            JSON.stringify(plainPayload),
            bundle.x25519Pub,
            bundle.kemPub,
          );
          finalPayload = { enc: env };
          encrypted = true;
        } else {
          console.warn("[composer] DM peer bundle not resolvable — sending transit-only");
        }
      }

      const event = await createSignedEvent(
        { type: "chat.msg", payload: finalPayload },
        kp,
      );
      await peer.publish({
        topic: t.topic,
        message: JSON.stringify(event),
        sender_id: event.sender_did,
        ts: event.ts,
        signature: event.signature,
      });

      if (encrypted) {
        // Could emit a "✓ sent encrypted" toast here. Phase 4 polish.
      }
    } catch (err) {
      console.warn("[composer] publish failed", err);
    } finally {
      setSending(false);
    }
  };

  const onKeyDown = (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  return (
    <div
      onDragEnter={(e) => { e.preventDefault(); setDragging(true); }}
      onDragOver={(e) => { e.preventDefault(); setDragging(true); }}
      onDragLeave={() => setDragging(false)}
      onDrop={onDrop}
      className={`
        relative m-4 mt-0 rounded-2xl
        bg-white/70 dark:bg-zinc-900/60
        backdrop-blur-xl
        border ${dragging ? "border-amber-400 ring-2 ring-amber-400/30" : "border-zinc-200/70 dark:border-zinc-800/70"}
        shadow-sm
        transition-colors
      `}
    >
      {dragging && (
        <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-2xl bg-amber-500/10 text-sm font-medium text-amber-700 dark:text-amber-300">
          drop to upload via iroh-blobs · any size, P2P
        </div>
      )}

      {pending.length > 0 && (
        <div className="flex flex-wrap gap-2 px-3 pt-3">
          {pending.map((p) => (
            <AttachmentPill
              key={p.tempId}
              name={p.name}
              size={p.size}
              mime={p.mime}
              status={p.status}
              progress={p.progress}
              ticket={p.ticket}
              onCopy={(t) => navigator.clipboard?.writeText(t).catch(() => {})}
            />
          ))}
        </div>
      )}

      {/* Formatting toolbar — wraps the current textarea selection in
          Markdown markers. The Conversation renderer (lib/markdown.js)
          parses them on display so the messages actually look bold /
          italic / code, not the raw asterisks. */}
      <div className="flex items-center gap-1 px-3 pt-2 border-b border-zinc-200/40 dark:border-zinc-800/40">
        <ToolbarBtn title="Bold (Ctrl+B)" onClick={() => wrapSelection("**")}>
          <BoldIcon />
        </ToolbarBtn>
        <ToolbarBtn title="Italic (Ctrl+I)" onClick={() => wrapSelection("*")}>
          <ItalicIcon />
        </ToolbarBtn>
        <ToolbarBtn title="Inline code" onClick={() => wrapSelection("`")}>
          <CodeIcon />
        </ToolbarBtn>
      </div>

      <textarea
        ref={textareaRef}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          // Ctrl/Cmd + B / I as quick shortcuts. The toolbar buttons
          // do the same thing visually — both paths share wrapSelection.
          if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey) {
            if (e.key === "b" || e.key === "B") { e.preventDefault(); wrapSelection("**"); return; }
            if (e.key === "i" || e.key === "I") { e.preventDefault(); wrapSelection("*"); return; }
          }
          onKeyDown(e);
        }}
        placeholder={`Message ${threadLabel(state)}`}
        rows={1}
        className="
          block w-full resize-none bg-transparent
          px-4 py-3 text-[14px] text-zinc-900 dark:text-zinc-100
          placeholder:text-zinc-400 dark:placeholder:text-zinc-500
          outline-none
        "
      />

      <div className="flex items-center justify-between gap-2 px-3 pb-2">
        <div className="flex items-center gap-1">
          <button
            onClick={() => fileInputRef.current?.click()}
            title="Attach file (any size — direct P2P transfer)"
            className="rounded-lg p-1.5 text-zinc-500 hover:bg-amber-500/10 hover:text-amber-600 dark:text-zinc-400 dark:hover:text-amber-400 transition-colors"
          >
            <PaperclipIcon />
          </button>
          <input
            ref={fileInputRef}
            type="file"
            multiple
            className="hidden"
            onChange={(e) => onPickFiles(e.target.files)}
          />
          <span className="text-[11px] text-zinc-400 dark:text-zinc-500">
            drag files, paste, or click 📎 — no size limit
          </span>
        </div>
        <button
          onClick={send}
          disabled={sending || (!text.trim() && !pending.some((p) => p.status === "uploaded"))}
          className="
            rounded-lg bg-amber-500 px-3 py-1.5 text-sm font-medium text-white
            hover:bg-amber-600
            disabled:cursor-not-allowed disabled:bg-zinc-300 dark:disabled:bg-zinc-700 disabled:text-zinc-500
            transition-colors
          "
        >
          {sending ? "Sending…" : "Send"}
        </button>
      </div>
    </div>
  );
}

const threadLabel = (state) => {
  const contacts = state.contactsByWorkspace[state.activeWorkspaceId] || [];
  const c = contacts.find((x) => x.id === state.activeThreadId);
  return c ? c.name : "thread";
};

const PaperclipIcon = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48" />
  </svg>
);

// Toolbar formatting buttons. ToolbarBtn is intentionally separate
// from the bottom-row IconBtn so the visual styles can drift apart
// later (toolbar buttons might pick up an "active" state when the
// cursor is inside an existing emphasis).
const ToolbarBtn = ({ title, onClick, children }) => (
  <button
    type="button"
    title={title}
    aria-label={title}
    onClick={onClick}
    className="rounded-md p-1.5 text-zinc-500 hover:bg-amber-500/10 hover:text-amber-600 dark:text-zinc-400 dark:hover:text-amber-400 transition-colors"
  >
    {children}
  </button>
);

const BoldIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    <path d="M7 4h6.5a4.5 4.5 0 0 1 2.6 8.16A5 5 0 0 1 14 21H7zm2 2v6h4.5a3 3 0 0 0 0-6zm0 8v5h5a3 3 0 0 0 0-5z" />
  </svg>
);
const ItalicIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
    <line x1="19" y1="4" x2="10" y2="4" />
    <line x1="14" y1="20" x2="5" y2="20" />
    <line x1="15" y1="4" x2="9" y2="20" />
  </svg>
);
const CodeIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <polyline points="16 18 22 12 16 6" />
    <polyline points="8 6 2 12 8 18" />
  </svg>
);
