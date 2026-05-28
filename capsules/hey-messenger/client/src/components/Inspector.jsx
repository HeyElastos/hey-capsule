import { useStore } from "../state/store.jsx";

const formatBytes = (n) => {
  if (n == null) return "";
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
};

const Section = ({ title, children }) => (
  <div className="px-4 py-3 border-b border-zinc-200/60 dark:border-zinc-800/60">
    <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
      {title}
    </div>
    {children}
  </div>
);

export default function Inspector() {
  const { state } = useStore();
  const messages = state.messages[state.activeThreadId] || [];
  const attachments = messages.flatMap((m) =>
    (m.payload?.attachments || []).map((a) => ({ ...a, from: m.sender_name, ts: m.ts }))
  );
  const participants = uniqueParticipants(messages, state.currentUser);
  const activeThreadName = labelFor(state);

  return (
    <aside
      className="
        w-72 shrink-0 hidden lg:flex flex-col
        bg-white/50 dark:bg-zinc-900/40
        backdrop-blur-xl
        border-l border-zinc-200/60 dark:border-zinc-800/60
        overflow-y-auto
      "
    >
      <div className="px-4 py-3 border-b border-zinc-200/60 dark:border-zinc-800/60">
        <div className="text-sm font-semibold tracking-tight">{activeThreadName}</div>
        <div className="text-[11px] text-zinc-500 dark:text-zinc-400">{participants.length} participants · {attachments.length} files</div>
      </div>

      <Section title="Shared files">
        {attachments.length === 0 ? (
          <div className="text-[12px] text-zinc-500 dark:text-zinc-400">No files shared yet.</div>
        ) : (
          <ul className="space-y-2">
            {attachments.map((a, i) => (
              <li key={a.cid || a.ticket || i} className="flex items-start gap-2 rounded-lg p-2 hover:bg-zinc-100/60 dark:hover:bg-zinc-800/40">
                <div className="text-lg leading-none mt-0.5">📎</div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px] font-medium text-zinc-900 dark:text-zinc-50">{a.name}</div>
                  <div className="text-[11px] text-zinc-500 dark:text-zinc-400">
                    {formatBytes(a.size)} · from {a.from}
                  </div>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Participants">
        <ul className="space-y-1.5">
          {participants.map((p) => (
            <li key={p.did} className="flex items-center gap-2 text-[13px]">
              <span className="h-2 w-2 rounded-full bg-emerald-500" />
              <span className="text-zinc-800 dark:text-zinc-200">{p.name}</span>
            </li>
          ))}
        </ul>
      </Section>

      <Section title="Transport">
        <ul className="space-y-1 text-[12px] text-zinc-600 dark:text-zinc-400">
          <li>Text: Carrier gossip (P2P)</li>
          <li>Files: iroh-blobs direct (P2P, unlimited)</li>
          <li>Calls: WebRTC P2P · Carrier signaling</li>
        </ul>
      </Section>
    </aside>
  );
}

const uniqueParticipants = (messages, me) => {
  const out = new Map();
  out.set(me.did, { did: me.did, name: "You" });
  for (const m of messages) {
    if (!out.has(m.sender_did)) out.set(m.sender_did, { did: m.sender_did, name: m.sender_name });
  }
  return Array.from(out.values());
};

const labelFor = (state) => {
  const contacts = state.contactsByWorkspace[state.activeWorkspaceId] || [];
  const c = contacts.find((x) => x.id === state.activeThreadId);
  return c ? c.name : "thread";
};
