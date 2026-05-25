import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Link } from "react-router-dom";
import {
  followUser,
  getUserById,
  unfollowUser,
} from "../api/auth";
import { CloseIcon, SearchIcon } from "./icons";

const extractUserId = (input) => {
  const trimmed = (input || "").trim();
  if (!trimmed) return null;
  const fromUrl = trimmed.match(/profile\/([0-9a-f-]{8,})/i);
  if (fromUrl) return fromUrl[1];
  const uuid = trimmed.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i);
  if (uuid) return uuid[0];
  if (/^[0-9a-f-]{8,}$/i.test(trimmed)) return trimmed;
  return null;
};

const Avatar = ({ name, avatar }) => {
  if (avatar) {
    return (
      <img
        src={avatar}
        alt=""
        className="h-28 w-28 flex-none rounded-2xl object-cover ring-2 ring-white/20 shadow-lg"
      />
    );
  }
  return (
    <div className="flex h-28 w-28 flex-none items-center justify-center rounded-2xl bg-gradient-to-br from-accent to-amber-600 text-3xl font-black text-accent-text shadow-lg">
      {(name || "?").slice(0, 2).toUpperCase()}
    </div>
  );
};

const SearchModal = ({ token, onClose, onChange }) => {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState(null);
  const [loading, setLoading] = useState(false);
  const [notFound, setNotFound] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const handler = (event) => {
      if (event.key === "Escape") onClose?.();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  useEffect(() => {
    const id = extractUserId(query);
    if (!id) {
      setResult(null);
      setNotFound(false);
      return;
    }

    let active = true;
    setLoading(true);
    setNotFound(false);
    const timer = setTimeout(async () => {
      try {
        const data = await getUserById(id, token);
        if (active) {
          setResult(data);
          setNotFound(false);
        }
      } catch {
        if (active) {
          setResult(null);
          setNotFound(true);
        }
      } finally {
        if (active) setLoading(false);
      }
    }, 350);

    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [query, token]);

  const handleFollow = async () => {
    if (!result || busy) return;
    setBusy(true);
    try {
      const data = await followUser(result.user.id, token);
      setResult({ ...result, relationship: data.relationship });
      onChange?.();
    } finally {
      setBusy(false);
    }
  };

  const handleUnfollow = async () => {
    if (!result || busy) return;
    setBusy(true);
    try {
      const data = await unfollowUser(result.user.id, token);
      setResult({ ...result, relationship: data.relationship });
      onChange?.();
    } finally {
      setBusy(false);
    }
  };

  const renderAction = () => {
    if (!result) return null;
    if (result.relationship === "self") {
      return (
        <p className="mt-3 text-center text-xs text-muted">That's you.</p>
      );
    }
    if (result.relationship === "following") {
      return (
        <button
          type="button"
          onClick={handleUnfollow}
          disabled={busy}
          className="unfrost mt-3 w-full rounded-full border border-surface-border bg-white/5 px-4 py-2 text-sm font-medium text-primary transition hover:bg-white/10 disabled:opacity-50"
        >
          Following
        </button>
      );
    }
    if (result.relationship === "requested") {
      return (
        <button
          type="button"
          onClick={handleUnfollow}
          disabled={busy}
          className="unfrost mt-3 w-full rounded-full border border-surface-border bg-white/5 px-4 py-2 text-sm font-medium text-primary transition hover:bg-white/10 disabled:opacity-50"
        >
          Request sent — tap to cancel
        </button>
      );
    }
    return (
      <button
        type="button"
        onClick={handleFollow}
        disabled={busy}
        className="unfrost mt-3 w-full rounded-full bg-accent px-4 py-2 text-sm font-semibold text-accent-text shadow-lg shadow-slate-900/20 transition hover:bg-amber-300 disabled:opacity-50"
      >
        Send follow request
      </button>
    );
  };

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-start justify-center px-4 pt-24 animate-fade-in bg-black/35 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose?.();
      }}
    >
      <div
        role="dialog"
        aria-label="Find user"
        className="relative h-fit w-full max-w-md animate-pop-in space-y-4 rounded-3xl p-6 backdrop-blur-[80px] bg-white/95 ring-1 ring-white/70 shadow-[inset_0_1px_0_rgba(255,255,255,0.7),0_18px_40px_-10px_rgba(0,0,0,0.45)] dark:bg-neutral-900/95 dark:ring-white/15 dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_18px_40px_-10px_rgba(0,0,0,0.65)]"
      >
        <header className="flex items-center justify-between">
          <h2 className="text-lg font-bold text-primary">Find user</h2>
          <button
            type="button"
            onClick={onClose}
            className="icon-btn-ghost"
            aria-label="Close"
          >
            <CloseIcon className="h-4 w-4" />
          </button>
        </header>

        <p className="text-xs text-muted">
          Paste a friend's profile ID (or full profile URL) to find them. They share it via QR code on their profile.
        </p>

        <div className="relative">
          <input
            type="text"
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Paste user ID or profile URL..."
            className="frosted-input !pr-10 text-sm"
          />
          <SearchIcon className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted" />
        </div>

        {loading && (
          <p className="animate-fade-in text-xs text-muted">Searching…</p>
        )}

        {notFound && !loading && (
          <p className="animate-fade-in text-xs text-red-400">
            No user with that ID.
          </p>
        )}

        {result && !loading && (
          <div className="animate-fade-in rounded-2xl border border-white/15 bg-white/5 p-5">
            <div className="flex flex-col items-center text-center">
              <Avatar name={result.user.name} avatar={result.user.avatar} />
              <Link
                to={`/profile/${result.user.id}`}
                onClick={onClose}
                className="mt-4 text-lg font-bold text-primary transition hover:text-accent"
              >
                {result.user.name}
              </Link>
              {result.user.bio && (
                <p className="mt-1 line-clamp-3 text-sm leading-snug text-muted">
                  {result.user.bio}
                </p>
              )}
            </div>
            {renderAction()}
          </div>
        )}
      </div>
    </div>,
    document.body
  );
};

export default SearchModal;
