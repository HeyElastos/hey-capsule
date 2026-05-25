import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { signIn } from "../api/auth";
import { passkeySignin, passkeySupported } from "../api/passkey";
import { CloseIcon } from "./icons";

const SignInModal = ({ onClose, onSuccess }) => {
  const [authKey, setAuthKey] = useState("");
  const [error, setError] = useState(null);
  const [loading, setLoading] = useState(false);
  const [passkeyBusy, setPasskeyBusy] = useState(false);

  useEffect(() => {
    const handler = (event) => {
      if (event.key === "Escape" && !loading && !passkeyBusy) onClose?.();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [loading, passkeyBusy, onClose]);

  const finishSuccess = (data) => {
    const profile = {
      user: data.user,
      accessToken: data.accessToken,
      refreshToken: data.refreshToken,
    };
    localStorage.setItem("profile", JSON.stringify(profile));
    onSuccess?.(profile);
  };

  const handleSubmit = async (event) => {
    event.preventDefault();
    if (!authKey.trim()) return;
    setError(null);
    setLoading(true);
    try {
      const data = await signIn({ authKey: authKey.trim() });
      finishSuccess(data);
    } catch (err) {
      setError(err.response?.data?.message || "Unable to sign in.");
    } finally {
      setLoading(false);
    }
  };

  const handlePasskey = async () => {
    setError(null);
    setPasskeyBusy(true);
    try {
      const data = await passkeySignin();
      finishSuccess(data);
    } catch (err) {
      setError(err.response?.data?.message || err.message || "Passkey sign-in failed.");
    } finally {
      setPasskeyBusy(false);
    }
  };

  const busy = loading || passkeyBusy;
  const canUsePasskey = passkeySupported();

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
            <h2 className="text-base font-semibold text-primary">Sign in to Hey</h2>
            <p className="mt-1 text-xs text-muted">
              Use a passkey, hardware key, or paste your secret key.
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

        {canUsePasskey && (
          <>
            <button
              type="button"
              onClick={handlePasskey}
              disabled={busy}
              className="unfrost flex w-full items-center justify-center gap-2 rounded-full border border-black/15 bg-black/5 px-5 py-2.5 text-sm font-semibold text-primary transition hover:bg-black/10 disabled:opacity-50 dark:border-white/15 dark:bg-white/5 dark:hover:bg-white/10"
            >
              <svg viewBox="0 0 24 24" className="h-4 w-4 fill-current">
                <path d="M12 2a5 5 0 0 0-5 5v3H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8a2 2 0 0 0-2-2h-1V7a5 5 0 0 0-5-5Zm-3 8V7a3 3 0 0 1 6 0v3H9Z" />
              </svg>
              {passkeyBusy ? "Waiting for passkey..." : "Sign in with passkey"}
            </button>

            <div className="flex items-center gap-2 text-[10px] uppercase tracking-wider text-muted">
              <span className="h-px flex-1 bg-black/10 dark:bg-white/10" />
              or paste your key
              <span className="h-px flex-1 bg-black/10 dark:bg-white/10" />
            </div>
          </>
        )}

        <textarea
          value={authKey}
          onChange={(e) => setAuthKey(e.target.value)}
          rows={4}
          placeholder="Paste your key here"
          disabled={busy}
          className="frosted-input w-full font-mono text-xs disabled:opacity-50"
        />

        {error && (
          <p className="animate-fade-in text-sm text-red-500 dark:text-red-400">
            {error}
          </p>
        )}

        <button
          type="submit"
          disabled={busy || !authKey.trim()}
          className="unfrost w-full rounded-full bg-accent px-5 py-2.5 text-sm font-semibold text-accent-text transition hover:bg-amber-300 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {loading ? "Signing in..." : "Sign in with key"}
        </button>
      </form>
    </div>,
    document.body
  );
};

export default SignInModal;
