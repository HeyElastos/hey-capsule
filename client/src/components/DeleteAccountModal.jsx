import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { deleteAccount } from "../api/auth";
import { CloseIcon } from "./icons";

const DeleteAccountModal = ({ token, userName, onClose, onDeleted }) => {
  const [confirmText, setConfirmText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    const handler = (e) => {
      if (e.key === "Escape" && !busy) onClose?.();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [busy, onClose]);

  const canSubmit = confirmText.trim().toLowerCase() === "delete";

  const handleSubmit = async (e) => {
    e.preventDefault();
    if (!canSubmit || busy) return;
    setBusy(true);
    setError(null);
    try {
      await deleteAccount(token);
      onDeleted?.();
    } catch (err) {
      setError(err.response?.data?.message || "Could not delete account.");
      setBusy(false);
    }
  };

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center px-4 animate-fade-in bg-black/35 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget && !busy) onClose?.();
      }}
    >
      <form
        onSubmit={handleSubmit}
        className="relative h-fit w-full max-w-md space-y-4 rounded-3xl p-6 text-left animate-pop-in backdrop-blur-[80px] bg-white/95 ring-1 ring-white/70 shadow-[inset_0_1px_0_rgba(255,255,255,0.7),0_18px_40px_-10px_rgba(0,0,0,0.45)] dark:bg-neutral-900/95 dark:ring-white/15 dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_18px_40px_-10px_rgba(0,0,0,0.65)]"
      >
        <header className="flex items-start justify-between gap-3">
          <div>
            <h2 className="text-base font-semibold text-red-500 dark:text-red-400">
              Delete account
            </h2>
            <p className="mt-1 text-xs text-muted">
              {userName ? `${userName}, this` : "This"} cannot be undone. Your
              posts, photos, videos, comments, and reactions will be permanently
              removed.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            aria-label="Close"
            className="icon-btn-ghost flex-none"
          >
            <CloseIcon className="h-4 w-4" />
          </button>
        </header>

        <div className="space-y-2">
          <label className="text-xs uppercase tracking-wider text-muted">
            Type <span className="font-mono text-red-500 dark:text-red-400">delete</span> to confirm
          </label>
          <input
            type="text"
            value={confirmText}
            onChange={(e) => setConfirmText(e.target.value)}
            disabled={busy}
            autoComplete="off"
            placeholder="delete"
            className="frosted-input text-sm disabled:opacity-50"
          />
        </div>

        {error && (
          <p className="text-sm text-red-500 dark:text-red-400">{error}</p>
        )}

        <div className="flex items-center justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="unfrost rounded-full border border-black/10 bg-black/5 px-5 py-2 text-sm text-primary transition hover:bg-black/10 disabled:opacity-50 dark:border-white/15 dark:bg-white/5 dark:hover:bg-white/10"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!canSubmit || busy}
            className="unfrost rounded-full bg-red-500 px-5 py-2 text-sm font-semibold text-white transition hover:bg-red-600 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {busy ? "Deleting..." : "Delete forever"}
          </button>
        </div>
      </form>
    </div>,
    document.body
  );
};

export default DeleteAccountModal;
