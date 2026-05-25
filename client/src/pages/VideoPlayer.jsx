import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  addComment,
  deleteComment,
  getPost,
  reactToPost,
} from "../api/auth";
import { ChevronLeftIcon, CloseIcon, HeartIcon } from "../components/icons";

const LIKE_EMOJI = "❤️";

const timeAgo = (iso) => {
  if (!iso) return "";
  const s = Math.max(1, Math.floor((Date.now() - new Date(iso).getTime()) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  if (d < 7) return `${d}d ago`;
  return new Date(iso).toLocaleDateString();
};

const Avatar = ({ name, avatar, small = false }) => {
  const cls = small ? "h-9 w-9" : "h-10 w-10";
  if (avatar) {
    return (
      <img
        src={avatar}
        alt=""
        className={`${cls} flex-none rounded-full object-cover ring-1 ring-white/20`}
      />
    );
  }
  return (
    <div
      className={`${cls} flex flex-none items-center justify-center rounded-full bg-gradient-to-br from-amber-300 to-amber-600 text-sm font-bold text-slate-900`}
    >
      {(name || "?").slice(0, 2).toUpperCase()}
    </div>
  );
};

const VideoPlayer = () => {
  const { id } = useParams();
  const navigate = useNavigate();
  const [post, setPost] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [commentText, setCommentText] = useState("");
  const [commentBusy, setCommentBusy] = useState(false);
  const [reactBusy, setReactBusy] = useState(false);

  const profile = useMemo(
    () => JSON.parse(localStorage.getItem("profile") || "null"),
    []
  );
  const token = profile?.accessToken;
  const currentUserId = profile?.user?.id;

  useEffect(() => {
    let active = true;
    setLoading(true);
    (async () => {
      try {
        const data = await getPost(id, token);
        if (active) setPost(data.post);
      } catch (e) {
        if (active) {
          setError(e.response?.data?.message || "Video not found.");
        }
      } finally {
        if (active) setLoading(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [id, token]);

  const videoItem = post?.images?.[0];
  const videoSrc = videoItem?.url;

  const likeIds = post?.reactions?.[LIKE_EMOJI] || [];
  const likeCount = likeIds.length;
  const youLiked = currentUserId ? likeIds.includes(currentUserId) : false;

  const totalReactions = useMemo(() => {
    if (!post?.reactions) return 0;
    return Object.values(post.reactions).reduce((sum, ids) => sum + ids.length, 0);
  }, [post]);

  const comments = post?.comments || [];

  const toggleLike = async () => {
    if (!token) {
      setError("Sign in to react.");
      return;
    }
    setReactBusy(true);
    try {
      const data = await reactToPost(post.id, LIKE_EMOJI, token);
      setPost(data.post);
    } catch (e) {
      setError(e.response?.data?.message || "Could not react.");
    } finally {
      setReactBusy(false);
    }
  };

  const submitComment = async (event) => {
    event.preventDefault();
    const text = commentText.trim();
    if (!text || !token) return;
    setCommentBusy(true);
    try {
      const data = await addComment(post.id, text, token);
      setPost(data.post);
      setCommentText("");
    } catch (e) {
      setError(e.response?.data?.message || "Could not post comment.");
    } finally {
      setCommentBusy(false);
    }
  };

  const removeComment = async (commentId) => {
    if (!token) return;
    try {
      const data = await deleteComment(post.id, commentId, token);
      setPost(data.post);
    } catch {
      /* noop */
    }
  };

  if (loading) {
    return (
      <div className="mx-auto max-w-4xl space-y-4">
        <div className="aspect-video w-full image-skeleton rounded-2xl" />
        <div className="h-6 w-2/3 image-skeleton rounded" />
        <div className="h-4 w-1/3 image-skeleton rounded" />
      </div>
    );
  }

  if (error || !post) {
    return (
      <div className="mx-auto max-w-md frosted-card animate-fade-up p-8 text-center">
        <h2 className="text-xl font-bold text-primary">Video unavailable</h2>
        <p className="mt-3 text-sm text-muted">{error || "Video not found."}</p>
        <Link
          to="/videos"
          className="unfrost mt-5 inline-block rounded-full bg-accent px-5 py-2.5 text-sm font-semibold text-accent-text"
        >
          Back to videos
        </Link>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-4xl space-y-6">
      <button
        type="button"
        onClick={() => navigate(-1)}
        className="unfrost inline-flex items-center gap-1.5 text-sm text-muted transition hover:text-primary"
      >
        <ChevronLeftIcon className="h-4 w-4" />
        Back
      </button>

      <div className="overflow-hidden rounded-2xl bg-black shadow-xl shadow-slate-950/40">
        {videoSrc ? (
          <video
            key={videoSrc}
            src={videoSrc}
            controls
            autoPlay
            playsInline
            className="aspect-video w-full bg-black"
          />
        ) : (
          <div className="flex aspect-video w-full items-center justify-center bg-gradient-to-br from-indigo-500 via-fuchsia-600 to-rose-500 text-white">
            <div className="flex h-20 w-20 items-center justify-center rounded-full bg-black/40 ring-1 ring-white/30 backdrop-blur-sm">
              <svg viewBox="0 0 24 24" className="ml-1 h-8 w-8 fill-current">
                <path d="M8 5v14l11-7z" />
              </svg>
            </div>
          </div>
        )}
      </div>

      <div className="space-y-3">
        <h1 className="text-xl font-semibold leading-snug text-primary sm:text-2xl">
          {post.caption || "Untitled"}
        </h1>

        <div className="flex flex-wrap items-center justify-between gap-3">
          <Link
            to={`/profile/${post.userId}`}
            className="unfrost flex items-center gap-3 transition hover:opacity-90"
          >
            <Avatar name={post.userName} avatar={post.userAvatar} size={10} />
            <div>
              <p className="text-sm font-semibold text-primary">
                {post.userName || "Unknown"}
              </p>
              <p className="text-xs text-muted">
                {timeAgo(post.createdAt)}
                {totalReactions > 0 && ` · ${totalReactions} reactions`}
              </p>
            </div>
          </Link>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={toggleLike}
              disabled={reactBusy}
              className={`unfrost flex items-center gap-2 rounded-full border px-4 py-2 text-sm font-medium transition disabled:opacity-50 ${
                youLiked
                  ? "border-rose-500/40 bg-rose-500/15 text-rose-500 dark:text-rose-400"
                  : "border-black/10 bg-black/5 text-primary hover:bg-black/10 dark:border-white/15 dark:bg-white/5 dark:hover:bg-white/10"
              }`}
              aria-pressed={youLiked}
              aria-label={youLiked ? "Unlike" : "Like"}
            >
              <HeartIcon className={`h-5 w-5 ${youLiked ? "fill-current" : ""}`} />
              <span>{likeCount}</span>
            </button>
          </div>
        </div>
      </div>

      <section className="space-y-4 border-t border-black/10 pt-6 dark:border-white/10">
        <h2 className="text-base font-semibold text-primary">
          {comments.length} {comments.length === 1 ? "comment" : "comments"}
        </h2>

        {token ? (
          <form onSubmit={submitComment} className="flex items-start gap-3">
            <Avatar
              name={profile?.user?.name}
              avatar={profile?.user?.avatar}
                          />
            <div className="flex-1 space-y-2">
              <textarea
                value={commentText}
                onChange={(e) => setCommentText(e.target.value)}
                placeholder="Add a comment..."
                rows={2}
                disabled={commentBusy}
                className="frosted-input w-full text-sm disabled:opacity-50"
                maxLength={500}
              />
              <div className="flex items-center justify-end gap-2">
                {commentText && (
                  <button
                    type="button"
                    onClick={() => setCommentText("")}
                    disabled={commentBusy}
                    className="unfrost rounded-full border border-black/10 bg-black/5 px-4 py-1.5 text-xs text-primary transition hover:bg-black/10 disabled:opacity-50 dark:border-white/15 dark:bg-white/5 dark:hover:bg-white/10"
                  >
                    Cancel
                  </button>
                )}
                <button
                  type="submit"
                  disabled={commentBusy || !commentText.trim()}
                  className="unfrost rounded-full bg-accent px-4 py-1.5 text-xs font-semibold text-accent-text transition hover:bg-amber-300 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {commentBusy ? "Posting..." : "Comment"}
                </button>
              </div>
            </div>
          </form>
        ) : (
          <p className="text-sm text-muted">
            <button
              type="button"
              onClick={() => window.dispatchEvent(new CustomEvent("open-signin"))}
              className="unfrost text-accent hover:underline"
            >
              Sign in
            </button>{" "}
            to comment.
          </p>
        )}

        {comments.length === 0 ? (
          <p className="py-8 text-center text-sm text-muted">
            No comments yet — be the first.
          </p>
        ) : (
          <ul className="space-y-4">
            {[...comments]
              .sort((a, b) => new Date(b.createdAt) - new Date(a.createdAt))
              .map((c) => {
                const isMine = c.userId === currentUserId;
                return (
                  <li key={c.id} className="group flex items-start gap-3">
                    <Avatar name={c.userName} avatar={c.userAvatar} small />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline gap-2">
                        <Link
                          to={`/profile/${c.userId}`}
                          className="unfrost text-sm font-semibold text-primary transition hover:underline"
                        >
                          {c.userName || "Unknown"}
                        </Link>
                        <span className="text-[10px] uppercase tracking-wider text-muted">
                          {timeAgo(c.createdAt)}
                        </span>
                      </div>
                      <p className="mt-0.5 whitespace-pre-wrap text-sm leading-snug text-primary">
                        {c.text}
                      </p>
                    </div>
                    {isMine && (
                      <button
                        type="button"
                        onClick={() => removeComment(c.id)}
                        className="icon-btn-ghost flex-none opacity-0 transition-opacity group-hover:opacity-100"
                        aria-label="Delete comment"
                      >
                        <CloseIcon className="h-3.5 w-3.5" />
                      </button>
                    )}
                  </li>
                );
              })}
          </ul>
        )}
      </section>
    </div>
  );
};

export default VideoPlayer;
