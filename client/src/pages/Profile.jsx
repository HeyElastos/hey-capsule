import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  followUser,
  getUserById,
  getUserPosts,
  unfollowUser,
  updateProfile,
} from "../api/auth";
import { useReveal } from "../hooks/useReveal";
import { CameraIcon, CommentIcon, HeartIcon } from "../components/icons";
import ProfileEditModal from "../components/ProfileEditModal";
import QRBadge from "../components/QRBadge";
import DeleteAccountModal from "../components/DeleteAccountModal";
import { passkeyAttach, passkeySupported } from "../api/passkey";

const GridTile = ({ post, index }) => {
  const { ref, visible } = useReveal();
  const coverItem = post.images?.[0];
  const cover = coverItem?.url;
  const coverIsVideo = coverItem?.type === "video";
  const reactionCount = useMemo(
    () =>
      Object.values(post.reactions || {}).reduce(
        (sum, ids) => sum + ids.length,
        0
      ),
    [post.reactions]
  );

  return (
    <Link
      ref={ref}
      to={`/p/${post.id}`}
      className={`group relative aspect-square overflow-hidden rounded-2xl frosted-card p-0 reveal ${visible ? "is-visible" : ""}`}
      style={{ transitionDelay: `${Math.min(index * 30, 240)}ms` }}
    >
      {cover ? (
        coverIsVideo ? (
          <video
            src={cover}
            muted
            playsInline
            preload="metadata"
            className="absolute inset-0 h-full w-full object-cover transition-transform duration-500 group-hover:scale-110"
          />
        ) : (
          <img
            src={cover}
            alt=""
            loading="lazy"
            className="absolute inset-0 h-full w-full object-cover transition-transform duration-500 group-hover:scale-110"
          />
        )
      ) : (
        <div className="absolute inset-0 bg-gradient-to-br from-slate-700 to-slate-900" />
      )}
      <div className="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100" />
      {post.images?.length > 1 && (
        <span className="absolute right-2 top-2 rounded-full bg-black/55 px-1.5 py-0.5 text-xs text-white backdrop-blur-md">
          {post.images.length}
        </span>
      )}
      <div className="absolute inset-x-0 bottom-0 flex items-center gap-3 p-3 text-xs text-white opacity-0 transition-opacity duration-300 group-hover:opacity-100">
        <span className="flex items-center gap-1">
          <HeartIcon className="h-3.5 w-3.5" />
          {reactionCount}
        </span>
        <span className="flex items-center gap-1">
          <CommentIcon className="h-3.5 w-3.5" />
          {post.comments?.length || 0}
        </span>
      </div>
    </Link>
  );
};

const VideoTile = ({ post, index }) => {
  const { ref, visible } = useReveal();
  const cover = post.images?.[0];
  const reactionCount = useMemo(
    () =>
      Object.values(post.reactions || {}).reduce(
        (sum, ids) => sum + ids.length,
        0
      ),
    [post.reactions]
  );

  return (
    <Link
      ref={ref}
      to={`/p/${post.id}`}
      className={`group relative aspect-[9/16] overflow-hidden rounded-2xl frosted-card p-0 reveal ${visible ? "is-visible" : ""}`}
      style={{ transitionDelay: `${Math.min(index * 30, 240)}ms` }}
    >
      {cover?.url ? (
        <video
          src={cover.url}
          muted
          playsInline
          preload="metadata"
          className="absolute inset-0 h-full w-full object-cover transition-transform duration-500 group-hover:scale-110"
        />
      ) : (
        <div className="absolute inset-0 bg-gradient-to-br from-indigo-500 via-fuchsia-600 to-rose-500" />
      )}
      <div className="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-transparent" />
      <div className="absolute inset-0 flex items-center justify-center opacity-90 transition group-hover:opacity-100">
        <div className="flex h-12 w-12 items-center justify-center rounded-full bg-black/45 ring-1 ring-white/30 backdrop-blur-sm">
          <svg viewBox="0 0 24 24" className="ml-0.5 h-5 w-5 fill-current text-white">
            <path d="M8 5v14l11-7z" />
          </svg>
        </div>
      </div>
      <div className="absolute inset-x-0 bottom-0 flex items-center justify-between gap-2 p-3 text-xs text-white">
        <span className="line-clamp-1 font-medium">
          {post.caption || "Untitled"}
        </span>
        <span className="flex flex-none items-center gap-1">
          <HeartIcon className="h-3.5 w-3.5" />
          {reactionCount}
        </span>
      </div>
    </Link>
  );
};

const Avatar = ({ name, avatar }) => {
  if (avatar) {
    return (
      <img
        src={avatar}
        alt=""
        className="h-28 w-28 rounded-full object-cover shadow-2xl shadow-slate-900/30 ring-4 ring-white/10"
      />
    );
  }
  return (
    <div className="flex h-28 w-28 items-center justify-center rounded-full bg-gradient-to-br from-amber-300 to-amber-600 text-4xl font-black text-accent-text shadow-2xl shadow-slate-900/30 ring-4 ring-white/10">
      {(name || "?").slice(0, 2).toUpperCase()}
    </div>
  );
};

const EditableAvatar = ({ name, avatar, busy, onPick }) => {
  const initials = (name || "?").slice(0, 2).toUpperCase();
  return (
    <button
      type="button"
      onClick={onPick}
      disabled={busy}
      aria-label="Change profile photo"
      className="unfrost group relative h-28 w-28 overflow-hidden rounded-full shadow-2xl shadow-slate-900/30 ring-4 ring-white/10 transition hover:ring-white/30 disabled:opacity-70"
    >
      {avatar ? (
        <img
          src={avatar}
          alt=""
          className="absolute inset-0 h-full w-full object-cover"
        />
      ) : (
        <div className="absolute inset-0 flex items-center justify-center bg-gradient-to-br from-amber-300 to-amber-600 text-4xl font-black text-accent-text">
          {initials}
        </div>
      )}
      <div className="absolute inset-0 flex items-center justify-center bg-black/55 opacity-0 transition-opacity duration-200 group-hover:opacity-100">
        <CameraIcon className="h-7 w-7 text-white" />
      </div>
      {busy && (
        <div className="absolute inset-0 flex items-center justify-center bg-black/55">
          <span className="h-6 w-6 animate-spin rounded-full border-2 border-white/30 border-t-white" />
        </div>
      )}
    </button>
  );
};

const Profile = () => {
  const { userId } = useParams();
  const [profile, setProfile] = useState(null);
  const [viewedUser, setViewedUser] = useState(null);
  const [relationship, setRelationship] = useState(null);
  const [posts, setPosts] = useState([]);
  const [postsHidden, setPostsHidden] = useState(false);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState(false);
  const [followBusy, setFollowBusy] = useState(false);
  const [mode, setMode] = useState(
    () => localStorage.getItem("mode") || "photo"
  );
  const [avatarBusy, setAvatarBusy] = useState(false);
  const [avatarError, setAvatarError] = useState(null);
  const avatarInputRef = useRef(null);
  const [deleteModalOpen, setDeleteModalOpen] = useState(false);
  const [passkeyBusy, setPasskeyBusy] = useState(false);
  const [passkeyMsg, setPasskeyMsg] = useState(null);
  const navigate = useNavigate();

  const handleAddPasskey = async () => {
    if (!profile?.accessToken) return;
    setPasskeyBusy(true);
    setPasskeyMsg(null);
    try {
      await passkeyAttach(profile.accessToken);
      setPasskeyMsg({ kind: "ok", text: "Passkey added ✓" });
      setTimeout(() => setPasskeyMsg(null), 3000);
    } catch (err) {
      setPasskeyMsg({
        kind: "err",
        text: err.response?.data?.message || err.message || "Could not add passkey.",
      });
    } finally {
      setPasskeyBusy(false);
    }
  };

  useEffect(() => {
    const stored = localStorage.getItem("profile");
    if (stored) setProfile(JSON.parse(stored));
  }, []);

  useEffect(() => {
    const onStorage = (e) => {
      if (e.key === "mode" && e.newValue) setMode(e.newValue);
    };
    const onModeChange = (e) => {
      if (e.detail) setMode(e.detail);
    };
    window.addEventListener("storage", onStorage);
    window.addEventListener("modechange", onModeChange);
    return () => {
      window.removeEventListener("storage", onStorage);
      window.removeEventListener("modechange", onModeChange);
    };
  }, []);

  const viewedUserId = userId || profile?.user?.id;
  const isSelf = !userId || userId === profile?.user?.id;
  const token = profile?.accessToken;

  useEffect(() => {
    if (!viewedUserId) return;
    let active = true;
    setLoading(true);
    (async () => {
      try {
        const userData = await getUserById(viewedUserId, token);
        if (!active) return;
        setViewedUser(userData.user);
        setRelationship(userData.relationship);

        const postsData = await getUserPosts(viewedUserId, token);
        if (!active) return;
        setPosts(postsData.posts || []);
        setPostsHidden(!!postsData.private);
      } catch {
        if (active) {
          setViewedUser(null);
          setPosts([]);
        }
      } finally {
        if (active) setLoading(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [viewedUserId, token]);

  const handleFollow = async () => {
    if (!token) return;
    setFollowBusy(true);
    try {
      const data = await followUser(viewedUserId, token);
      setRelationship(data.relationship);
    } finally {
      setFollowBusy(false);
    }
  };

  const handleUnfollow = async () => {
    if (!token) return;
    setFollowBusy(true);
    try {
      const data = await unfollowUser(viewedUserId, token);
      setRelationship(data.relationship);
      setPosts([]);
      setPostsHidden(true);
    } finally {
      setFollowBusy(false);
    }
  };

  const displayName = viewedUser?.name || "User";
  const bio = viewedUser?.bio || "";
  const avatar = viewedUser?.avatar || "";
  const counts = viewedUser?.counts || { followers: 0, following: 0 };

  const { photoPosts, videoPosts } = useMemo(() => {
    const photo = [];
    const video = [];
    for (const p of posts) {
      if (p.images?.[0]?.type === "video") video.push(p);
      else photo.push(p);
    }
    return { photoPosts: photo, videoPosts: video };
  }, [posts]);

  const handleSaved = (updatedUser) => {
    if (!isSelf) return;
    const stored = JSON.parse(localStorage.getItem("profile") || "null");
    if (stored) {
      stored.user = { ...stored.user, ...updatedUser };
      localStorage.setItem("profile", JSON.stringify(stored));
      setProfile(stored);
    }
    setViewedUser((current) => (current ? { ...current, ...updatedUser } : current));
  };

  const handleAvatarPick = () => {
    if (avatarBusy) return;
    setAvatarError(null);
    avatarInputRef.current?.click();
  };

  const handleAvatarChange = async (event) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (!file.type.startsWith("image/")) {
      setAvatarError("Photo must be an image.");
      return;
    }
    if (file.size > 10 * 1024 * 1024) {
      setAvatarError("Photo is over 10MB.");
      return;
    }
    setAvatarError(null);
    setAvatarBusy(true);
    try {
      const data = await updateProfile({ avatar: file }, token);
      handleSaved(data.user);
    } catch (e) {
      setAvatarError(e.response?.data?.message || "Could not upload photo.");
    } finally {
      setAvatarBusy(false);
    }
  };

  if (!profile && isSelf) {
    return (
      <div className="mx-auto max-w-md frosted-card animate-fade-up p-8 text-center">
        <h2 className="text-2xl font-bold text-primary">Profile</h2>
        <p className="mt-3 text-muted">Sign in to view your profile.</p>
        <button
          type="button"
          onClick={() => window.dispatchEvent(new CustomEvent("open-signin"))}
          className="unfrost mt-5 inline-block rounded-full bg-accent px-5 py-2.5 text-sm font-semibold text-accent-text"
        >
          Sign in
        </button>
      </div>
    );
  }

  const renderFollowButton = () => {
    if (isSelf) return null;
    if (relationship === "following") {
      return (
        <button
          type="button"
          onClick={handleUnfollow}
          disabled={followBusy}
          className="unfrost rounded-full border border-surface-border bg-white/5 px-5 py-2 text-sm font-medium text-primary transition hover:bg-white/10 disabled:opacity-50"
        >
          Following
        </button>
      );
    }
    if (relationship === "requested") {
      return (
        <button
          type="button"
          onClick={handleUnfollow}
          disabled={followBusy}
          className="unfrost rounded-full border border-surface-border bg-white/5 px-5 py-2 text-sm font-medium text-primary transition hover:bg-white/10 disabled:opacity-50"
        >
          Requested
        </button>
      );
    }
    return (
      <button
        type="button"
        onClick={handleFollow}
        disabled={followBusy}
        className="unfrost rounded-full bg-accent px-5 py-2 text-sm font-semibold text-accent-text shadow-lg shadow-slate-900/20 transition hover:bg-amber-300 disabled:opacity-50"
      >
        Follow
      </button>
    );
  };

  return (
    <div className="mx-auto max-w-4xl space-y-8">
      <section className="flex flex-col items-center text-center animate-fade-up">
        {isSelf ? (
          <>
            <EditableAvatar
              name={displayName}
              avatar={avatar}
              busy={avatarBusy}
              onPick={handleAvatarPick}
            />
            <input
              ref={avatarInputRef}
              type="file"
              accept="image/*"
              onChange={handleAvatarChange}
              className="hidden"
            />
            {avatarError && (
              <p className="mt-2 text-xs text-red-500 dark:text-red-400">
                {avatarError}
              </p>
            )}
          </>
        ) : (
          <Avatar name={displayName} avatar={avatar} />
        )}

        <div className="mt-5 flex items-center justify-center gap-2">
          <h1 className="text-2xl font-bold text-primary">{displayName}</h1>
          {viewedUser?.id && (
            <QRBadge
              url={`${window.location.origin}/profile/${viewedUser.id}`}
              label={`${displayName}'s QR code`}
            />
          )}
        </div>

        {bio ? (
          <p className="mt-2 max-w-md whitespace-pre-line text-sm leading-6 text-muted">
            {bio}
          </p>
        ) : isSelf ? (
          <p className="mt-2 text-sm italic text-muted">
            No bio yet — add one to introduce yourself.
          </p>
        ) : null}

        <div className="mt-4 flex items-center gap-6 text-xs uppercase tracking-wider text-muted">
          <span>
            <span className="text-base font-semibold text-primary">
              {photoPosts.length}
            </span>{" "}
            photos
          </span>
          <span>
            <span className="text-base font-semibold text-primary">
              {videoPosts.length}
            </span>{" "}
            videos
          </span>
          <span>
            <span className="text-base font-semibold text-primary">
              {counts.followers}
            </span>{" "}
            followers
          </span>
          <span>
            <span className="text-base font-semibold text-primary">
              {counts.following}
            </span>{" "}
            following
          </span>
        </div>

        <div className="mt-5 flex flex-wrap items-center justify-center gap-6">
          {isSelf ? (
            <>
              <button
                type="button"
                onClick={() => setEditing(true)}
                className="unfrost rounded-full border border-surface-border bg-white/5 px-5 py-2 text-sm font-medium text-primary transition hover:bg-white/10"
              >
                Edit profile
              </button>
              {passkeySupported() && (
                <button
                  type="button"
                  onClick={handleAddPasskey}
                  disabled={passkeyBusy}
                  className="unfrost rounded-full border border-surface-border bg-white/5 px-5 py-2 text-sm font-medium text-primary transition hover:bg-white/10 disabled:opacity-50"
                >
                  {passkeyBusy ? "Waiting for passkey..." : "Add a passkey"}
                </button>
              )}
              <button
                type="button"
                onClick={() => setDeleteModalOpen(true)}
                style={{ backgroundColor: "rgb(239 68 68)" }}
                className="rounded-full border-2 border-red-600 px-5 py-2 text-sm font-semibold text-white shadow-md shadow-red-900/30 transition hover:!bg-red-600"
              >
                Delete account
              </button>
            </>
          ) : (
            renderFollowButton()
          )}
        </div>

        {passkeyMsg && (
          <p
            className={`mt-3 text-xs ${
              passkeyMsg.kind === "ok"
                ? "text-emerald-600 dark:text-emerald-400"
                : "text-red-500 dark:text-red-400"
            }`}
          >
            {passkeyMsg.text}
          </p>
        )}
      </section>

      <section className="animate-fade-up space-y-4" style={{ animationDelay: "120ms" }}>
        {loading ? (
          mode === "video" ? (
            <div className="grid grid-cols-2 gap-6 sm:grid-cols-3 sm:gap-8">
              {[0, 1, 2, 3, 4, 5].map((i) => (
                <div key={i} className="aspect-[9/16] rounded-2xl image-skeleton" />
              ))}
            </div>
          ) : (
            <div className="grid grid-cols-3 gap-6 sm:gap-8">
              {[0, 1, 2, 3, 4, 5].map((i) => (
                <div key={i} className="aspect-square rounded-2xl image-skeleton" />
              ))}
            </div>
          )
        ) : postsHidden ? (
          <div className="frosted-card animate-fade-up p-10 text-center">
            <div className="mx-auto flex h-14 w-14 items-center justify-center rounded-full bg-white/10">
              <svg viewBox="0 0 24 24" className="h-6 w-6 fill-current text-muted">
                <path d="M12 2a5 5 0 0 0-5 5v3H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8a2 2 0 0 0-2-2h-1V7a5 5 0 0 0-5-5Zm-3 8V7a3 3 0 0 1 6 0v3H9Z" />
              </svg>
            </div>
            <p className="mt-4 text-base font-semibold text-primary">
              This profile is private
            </p>
            <p className="mt-1 text-sm text-muted">
              Follow {displayName} to see their posts.
            </p>
          </div>
        ) : mode === "video" ? (
          videoPosts.length === 0 ? (
            <div className="frosted-card py-12 text-center">
              <p className="text-sm text-muted">
                {isSelf ? "Your videos will appear here." : "No videos yet."}
              </p>
              {isSelf && (
                <Link
                  to="/posts"
                  className="unfrost mt-4 inline-block rounded-full bg-accent px-5 py-2.5 text-sm font-semibold text-accent-text transition hover:bg-amber-300"
                >
                  Share your first video
                </Link>
              )}
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-6 sm:grid-cols-3 sm:gap-8">
              {videoPosts.map((post, i) => (
                <VideoTile key={post.id} post={post} index={i} />
              ))}
            </div>
          )
        ) : photoPosts.length === 0 ? (
          <div className="frosted-card py-12 text-center">
            <p className="text-sm text-muted">
              {isSelf ? "Your photos will appear here." : "No photos yet."}
            </p>
            {isSelf && (
              <Link
                to="/posts"
                className="unfrost mt-4 inline-block rounded-full bg-accent px-5 py-2.5 text-sm font-semibold text-accent-text transition hover:bg-amber-300"
              >
                Share your first photo
              </Link>
            )}
          </div>
        ) : (
          <div className="grid grid-cols-3 gap-6 sm:gap-8">
            {photoPosts.map((post, i) => (
              <GridTile key={post.id} post={post} index={i} />
            ))}
          </div>
        )}
      </section>

      {editing && isSelf && (
        <ProfileEditModal
          user={profile?.user}
          token={profile?.accessToken}
          onClose={() => setEditing(false)}
          onSaved={handleSaved}
        />
      )}

      {deleteModalOpen && isSelf && (
        <DeleteAccountModal
          token={profile?.accessToken}
          userName={profile?.user?.name}
          onClose={() => setDeleteModalOpen(false)}
          onDeleted={() => {
            localStorage.removeItem("profile");
            navigate("/");
          }}
        />
      )}
    </div>
  );
};

export default Profile;
