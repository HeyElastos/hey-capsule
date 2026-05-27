// Honest encryption posture indicator for the chat header.
//
//   🔒 E2E       — Hybrid X25519 + ML-KEM-768 + ChaCha20-Poly1305.
//                  Used for DMs once at least one inbound message has
//                  been decrypted with our keys.
//   🔓 Transit   — Carrier QUIC (TLS 1.3) hop-to-hop. Bytes are
//                  encrypted in flight but anyone who joins the topic
//                  reads the plaintext. Used for groups (MLS pending)
//                  and for DMs before the peer's profile bundle has
//                  been received.

export default function EncryptionBadge({ kind }) {
  const e2e = kind === "e2e";
  return (
    <span
      title={
        e2e
          ? "Hybrid post-quantum end-to-end: X25519 + ML-KEM-768 + ChaCha20-Poly1305. Only the recipient can read."
          : "Encrypted hop-to-hop in transit (QUIC + TLS 1.3) but anyone in the topic can read the plaintext. Group end-to-end (MLS) is a tracked follow-up."
      }
      className={`flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider ${
        e2e
          ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
          : "bg-amber-400/15 text-amber-700 dark:text-amber-300"
      }`}
    >
      <span aria-hidden>{e2e ? "🔒" : "🔓"}</span>
      {e2e ? "E2E · PQ" : "transit"}
    </span>
  );
}
