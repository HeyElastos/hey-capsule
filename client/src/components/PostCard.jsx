import { useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router-dom";
import ImageCarousel from "./ImageCarousel";
import ReactionPicker from "./ReactionPicker";
import { useReveal } from "../hooks/useReveal";
import { CloseIcon, CommentIcon, PaperPlaneIcon, RepostIcon, SmileIcon, TrashIcon } from "./icons";
import {
  addComment as apiAddComment,
  deleteComment as apiDeleteComment,
  deletePost as apiDeletePost,
  reactToPost as apiReact,
  repostPost as apiRepost,
} from "../api/auth";

const formatCount = (n) => (n > 10 ? "10+" : String(n));

const timeAgo = (iso) => {
  if (!iso) return "";
  const seconds = Math.max(1, Math.floor((Date.now() - new Date(iso).getTime()) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  return new Date(iso).toLocaleDateString();
};

const Avatar = ({ name, avatar }) => {
  if (avatar) {
    return (
      <img
        src={avatar}
        alt=""
        className="h-12 w-12 flex-none rounded-full object-cover ring-1 ring-white/15 shadow-sm"
      />
    );
  }
  return (
    <div className="flex h-12 w-12 flex-none items-center justify-center rounded-full bg-gradient-to-br from-accent to-amber-600 text-base font-bold text-accent-text shadow-sm">
      {(name || "?").slice(0, 2).toUpperCase()}
    </div>
  );
};

const PostCard = ({ post, currentUser, token, onChange, onDelete }) => {
  const { ref, visible } = useReveal();
  const [commentText, setCommentText] = useState("");
  const [showAllComments, setShowAllComments] = useState(false);
  const [showCommentForm, setShowCommentForm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const commentInputRef = useRef(null);
  const [emojiOpen, setEmojiOpen] = useState(false);
  const emojiWrapRef = useRef(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const confirmDeleteRef = useRef(null);

  useEffect(() => {
    if (showCommentForm) {
      commentInputRef.current?.focus();
    } else {
      setEmojiOpen(false);
    }
  }, [showCommentForm]);

  useEffect(() => {
    if (!emojiOpen) return;
    const handler = (event) => {
      if (!emojiWrapRef.current?.contains(event.target)) setEmojiOpen(false);
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [emojiOpen]);

  useEffect(() => {
    if (!confirmDelete) return;
    const handler = (event) => {
      if (!confirmDeleteRef.current?.contains(event.target)) setConfirmDelete(false);
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [confirmDelete]);

  const insertEmoji = (emoji) => {
    const input = commentInputRef.current;
    if (!input) {
      setCommentText((current) => current + emoji);
      return;
    }
    const start = input.selectionStart ?? commentText.length;
    const end = input.selectionEnd ?? commentText.length;
    const next = commentText.slice(0, start) + emoji + commentText.slice(end);
    setCommentText(next);
    requestAnimationFrame(() => {
      input.focus();
      const caret = start + emoji.length;
      input.setSelectionRange(caret, caret);
    });
  };

  const COMMENT_EMOJIS = ["😂", "❤️", "🔥", "😍", "😮", "👏", "🙏", "😢", "💯", "✨", "👀", "🎉"];

  const isOwner = currentUser?.id === post.userId;
  const myReactions = useMemo(() => {
    const list = [];
    for (const [emoji, ids] of Object.entries(post.reactions || {})) {
      if (ids.includes(currentUser?.id)) list.push(emoji);
    }
    return list;
  }, [post.reactions, currentUser]);

  const reactionEntries = useMemo(() => {
    const entries = Object.entries(post.reactions || {});
    entries.sort((a, b) => b[1].length - a[1].length);
    return entries;
  }, [post.reactions]);

  const topReactionEmojis = useMemo(
    () => reactionEntries.slice(0, 3).map(([emoji]) => emoji),
    [reactionEntries]
  );

  const totalReactions = useMemo(
    () => reactionEntries.reduce((sum, [, ids]) => sum + ids.length, 0),
    [reactionEntries]
  );

  const didRepost = useMemo(
    () => (post.reposts || []).some((r) => r.userId === currentUser?.id),
    [post.reposts, currentUser]
  );

  const captionWithTags = useMemo(() => {
    if (!post.caption) return null;
    return post.caption.split(/(\s+)/).map((part, i) => {
      if (/^#[\p{L}\p{N}_]+/u.test(part)) {
        return (
          <span key={i} className="text-accent">
            {part}
          </span>
        );
      }
      return <span key={i}>{part}</span>;
    });
  }, [post.caption]);

  const runReact = async (emoji) => {
    if (!token) {
      setError("Sign in to react.");
      return;
    }
    setError(null);
    setBusy(true);
    try {
      const data = await apiReact(post.id, emoji, token);
      onChange?.(data.post);
    } catch (e) {
      setError(e.response?.data?.message || "Could not react.");
    } finally {
      setBusy(false);
    }
  };

  const runRepost = async () => {
    if (!token) {
      setError("Sign in to repost.");
      return;
    }
    setError(null);
    setBusy(true);
    try {
      const data = await apiRepost(post.id, token);
      onChange?.(data.post);
    } catch (e) {
      setError(e.response?.data?.message || "Could not repost.");
    } finally {
      setBusy(false);
    }
  };

  const submitComment = async (event) => {
    event.preventDefault();
    if (!commentText.trim() || !token) return;
    setError(null);
    setBusy(true);
    try {
      const data = await apiAddComment(post.id, commentText.trim(), token);
      onChange?.(data.post);
      setCommentText("");
    } catch (e) {
      setError(e.response?.data?.message || "Could not comment.");
    } finally {
      setBusy(false);
    }
  };

  const removeComment = async (commentId) => {
    if (!token) return;
    setBusy(true);
    try {
      const data = await apiDeleteComment(post.id, commentId, token);
      onChange?.(data.post);
    } catch (e) {
      setError(e.response?.data?.message || "Could not delete comment.");
    } finally {
      setBusy(false);
    }
  };

  const removePost = async () => {
    if (!token || !isOwner) return;
    setConfirmDelete(false);
    setBusy(true);
    try {
      await apiDeletePost(post.id, token);
      onDelete?.(post.id);
    } catch (e) {
      setError(e.response?.data?.message || "Could not delete.");
      setBusy(false);
    }
  };

  const visibleComments = showAllComments
    ? post.comments
    : (post.comments || []).slice(-2);

  return (
    <article
      ref={ref}
      className={`frosted-card overflow-hidden p-0 reveal ${visible ? "is-visible" : ""}`}
    >
      {(post.reposts || []).length > 0 && (
        <div className="flex items-center gap-2 border-b border-white/10 px-5 py-2.5 text-xs uppercase tracking-wider text-muted">
          <RepostIcon className="h-3.5 w-3.5" />
          <span>
            Reposted by {post.reposts[post.reposts.length - 1].userName}
            {post.reposts.length > 1 && ` +${post.reposts.length - 1}`}
          </span>
        </div>
      )}

      <header className="flex items-start gap-3 px-5 pt-4 pb-2">
        <Link to={`/profile/${post.userId}`} className="flex-none">
          <Avatar name={post.userName} avatar={post.userAvatar} />
        </Link>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <Link
              to={`/profile/${post.userId}`}
              className="font-semibold text-primary hover:text-accent transition-colors"
            >
              {post.userName}
            </Link>
            <span className="text-xs text-muted">·</span>
            <span className="text-xs text-muted">{timeAgo(post.createdAt)}</span>
          </div>
          {captionWithTags && (
            <p className="mt-1 whitespace-pre-line text-sm leading-6 text-primary">
              {captionWithTags}
            </p>
          )}
        </div>
        {isOwner && (
          <div ref={confirmDeleteRef} className="relative flex-none">
            <button
              type="button"
              onClick={() => setConfirmDelete((current) => !current)}
              disabled={busy}
              className={`icon-btn-ghost hover:text-red-400 disabled:opacity-50 ${
                confirmDelete ? "text-red-400" : ""
              }`}
              aria-label="Delete post"
              aria-expanded={confirmDelete}
            >
              <TrashIcon className="h-4 w-4" />
            </button>
            {confirmDelete && (
              <div
                role="dialog"
                aria-label="Confirm delete"
                className="absolute right-0 top-full z-20 mt-2 flex w-44 animate-fade-in items-center gap-1 rounded-xl border border-white/15 bg-neutral-900/95 p-1.5 shadow-xl backdrop-blur-xl"
              >
                <button
                  type="button"
                  onClick={removePost}
                  disabled={busy}
                  className="unfrost flex-1 rounded-lg bg-red-500 px-3 py-1.5 text-xs font-semibold text-white transition hover:bg-red-600 disabled:opacity-50"
                >
                  Delete
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmDelete(false)}
                  disabled={busy}
                  className="icon-btn-ghost !p-1.5"
                  aria-label="Cancel"
                >
                  <CloseIcon className="h-3.5 w-3.5" />
                </button>
              </div>
            )}
          </div>
        )}
      </header>

      {post.images?.length > 0 && (
        <div className="px-1 pb-1">
          <ImageCarousel images={post.images} />
        </div>
      )}

      <div className="px-5 pt-3">
        <div className="flex flex-wrap items-center gap-2">
          <ReactionPicker
            onPick={runReact}
            myReactions={myReactions}
            totalCount={totalReactions}
            topEmojis={topReactionEmojis}
            disabled={busy}
          />

          <button
            type="button"
            onClick={runRepost}
            disabled={busy}
            className={`unfrost reaction-chip ${didRepost ? "is-active" : ""}`}
            aria-label={didRepost ? "Undo repost" : "Repost"}
          >
            <RepostIcon className="h-5 w-5" />
            {(post.reposts?.length || 0) > 0 && (
              <span className="text-xs font-medium">{formatCount(post.reposts.length)}</span>
            )}
          </button>

          <button
            type="button"
            onClick={() => setShowCommentForm((current) => !current)}
            className={`unfrost reaction-chip ${showCommentForm ? "is-active" : ""}`}
            aria-label="Comment"
            aria-expanded={showCommentForm}
          >
            <CommentIcon className="h-5 w-5" />
            {(post.comments?.length || 0) > 0 && (
              <span className="text-xs font-medium">{formatCount(post.comments.length)}</span>
            )}
          </button>
        </div>

      </div>

      <div className="px-5 pb-4 pt-3">
        {(post.comments?.length || 0) > 2 && !showAllComments && (
          <button
            type="button"
            onClick={() => setShowAllComments(true)}
            className="unfrost text-xs text-muted hover:text-primary"
          >
            View all {post.comments.length} comments
          </button>
        )}

        <ul className="mt-2 space-y-2">
          {visibleComments?.map((comment) => {
            const canDelete =
              comment.userId === currentUser?.id || isOwner;
            return (
              <li
                key={comment.id}
                className="group flex items-start gap-2 text-sm leading-6"
              >
                <span
                  title={comment.userName}
                  aria-label={comment.userName}
                  className="flex-none mt-0.5"
                >
                  {comment.userAvatar ? (
                    <img
                      src={comment.userAvatar}
                      alt={comment.userName}
                      className="h-6 w-6 rounded-full object-cover ring-1 ring-white/15 dark:ring-white/15 ring-black/10 shadow-sm"
                    />
                  ) : (
                    <span className="flex h-6 w-6 items-center justify-center rounded-full bg-gradient-to-br from-accent to-amber-600 text-[10px] font-bold text-accent-text shadow-sm">
                      {(comment.userName || "?").slice(0, 2).toUpperCase()}
                    </span>
                  )}
                </span>
                <span className="flex-1 text-muted break-words">
                  {comment.text}
                </span>
                <span className="text-xs text-muted whitespace-nowrap">
                  {timeAgo(comment.createdAt)}
                </span>
                {canDelete && (
                  <button
                    type="button"
                    onClick={() => removeComment(comment.id)}
                    className="unfrost text-xs text-muted opacity-0 transition-opacity group-hover:opacity-100 hover:text-red-400"
                    aria-label="Delete comment"
                  >
                    ×
                  </button>
                )}
              </li>
            );
          })}
        </ul>

        {token && showCommentForm && (
          <form
            onSubmit={submitComment}
            className="mt-3 flex items-center gap-2 animate-fade-up"
          >
            <div
              ref={emojiWrapRef}
              className="relative flex-1"
            >
              <input
                ref={commentInputRef}
                type="text"
                value={commentText}
                onChange={(e) => setCommentText(e.target.value)}
                placeholder="Add a comment..."
                maxLength={500}
                className="frosted-input w-full !rounded-full !py-2 !pr-10 text-sm"
              />
              <button
                type="button"
                onClick={() => setEmojiOpen((current) => !current)}
                className="icon-btn-ghost absolute right-1 inset-y-1"
                aria-label="Insert emoji"
                aria-expanded={emojiOpen}
              >
                <SmileIcon className="h-4 w-4" />
              </button>

              {emojiOpen && (
                <div className="absolute bottom-full right-0 z-20 mb-2 flex animate-pop-in flex-wrap gap-1 rounded-2xl bg-black/65 p-2 shadow-2xl backdrop-blur-xl w-56">
                  {COMMENT_EMOJIS.map((emoji) => (
                    <button
                      key={emoji}
                      type="button"
                      onClick={() => insertEmoji(emoji)}
                      className="unfrost flex h-8 w-8 items-center justify-center rounded-full text-lg transition-transform duration-150 hover:scale-125 hover:bg-white/15"
                      aria-label={`Insert ${emoji}`}
                    >
                      {emoji}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <button
              type="submit"
              disabled={!commentText.trim() || busy}
              aria-label="Post comment"
              className="unfrost flex h-9 w-9 flex-none items-center justify-center rounded-full bg-accent text-accent-text shadow-sm transition disabled:opacity-50 hover:bg-amber-300"
            >
              <PaperPlaneIcon className="h-4 w-4 -translate-x-0.5 translate-y-0.5" />
            </button>
          </form>
        )}
        {!token && (
          <p className="mt-3 text-xs text-muted">
            <button
              type="button"
              onClick={() => window.dispatchEvent(new CustomEvent("open-signin"))}
              className="unfrost text-accent hover:underline"
            >
              Sign in
            </button>{" "}
            to react or comment.
          </p>
        )}

        {error && <p className="mt-2 text-xs text-red-400">{error}</p>}
      </div>
    </article>
  );
};

export default PostCard;
