import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { createPost } from "../api/auth";
import ImageDropzone from "../components/ImageDropzone";
import { PaperPlaneIcon } from "../components/icons";

const Posts = () => {
  const navigate = useNavigate();
  const [items, setItems] = useState([]);
  const [caption, setCaption] = useState("");
  const [loading, setLoading] = useState(false);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState(null);

  const profile = useMemo(
    () => JSON.parse(localStorage.getItem("profile") || "null"),
    []
  );
  const token = profile?.accessToken;
  const mode = useMemo(() => localStorage.getItem("mode") || "photo", []);
  const isVideo = mode === "video";

  useEffect(() => {
    if (!profile) {
      navigate("/");
      window.dispatchEvent(new CustomEvent("open-signin"));
    }
  }, [navigate, profile]);

  if (!profile) return null;

  const submit = async (event) => {
    event.preventDefault();
    if (items.length === 0) {
      setError(isVideo ? "Add a video." : "Add at least one image.");
      return;
    }
    setError(null);
    setLoading(true);
    setProgress(0);

    try {
      await createPost(
        { caption: caption.trim(), images: items.map((i) => i.file) },
        token,
        setProgress
      );
      navigate(isVideo ? "/videos" : "/");
    } catch (e) {
      setError(e.response?.data?.message || e.message || "Could not create post.");
    } finally {
      setLoading(false);
    }
  };

  const captionLen = caption.length;
  const captionMax = 2000;

  return (
    <div className="relative mx-auto max-w-3xl space-y-6">
      {isVideo ? (
        <svg
          aria-hidden="true"
          className="float-shape shape-gentle pointer-events-none absolute -top-10 right-2 text-amber-600/60 dark:text-amber-200/70 sm:-top-8 sm:right-6"
          style={{ width: 160, height: 100, overflow: "visible" }}
          viewBox="0 0 60 36"
          fill="none"
          stroke="currentColor"
          strokeWidth="1"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          {/* Cassette body */}
          <rect x="3.5" y="5" width="53" height="26" rx="2.5" />

          {/* Label area (top) */}
          <rect x="7" y="7.5" width="46" height="5" rx="0.8" strokeWidth="0.6" />
          <line x1="9" y1="10" x2="33" y2="10" strokeWidth="0.45" />
          <line x1="9" y1="11.3" x2="27" y2="11.3" strokeWidth="0.45" />

          {/* Bottom tape window cutout (where the head reads the tape) */}
          <line x1="20" y1="29" x2="40" y2="29" strokeWidth="0.6" />

          {/* Spindle/screw holes at corners */}
          <circle cx="7" cy="28.5" r="0.55" />
          <circle cx="53" cy="28.5" r="0.55" />

          {/* Tape between reels — top edge flows right, bottom flows left */}
          <line
            x1="24"
            y1="19"
            x2="36"
            y2="19"
            strokeWidth="0.8"
            strokeDasharray="2 1.5"
          >
            <animate
              attributeName="stroke-dashoffset"
              from="3.5"
              to="0"
              dur="0.9s"
              repeatCount="indefinite"
            />
          </line>
          <line
            x1="24"
            y1="23"
            x2="36"
            y2="23"
            strokeWidth="0.8"
            strokeDasharray="2 1.5"
          >
            <animate
              attributeName="stroke-dashoffset"
              from="0"
              to="3.5"
              dur="0.9s"
              repeatCount="indefinite"
            />
          </line>

          {/* Left reel */}
          <g>
            <circle cx="18" cy="21" r="5" />
            <circle cx="18" cy="21" r="1.3" fill="currentColor" stroke="none" />
            <circle cx="18" cy="17.3" r="0.75" />
            <circle cx="18" cy="24.7" r="0.75" />
            <circle cx="14.3" cy="21" r="0.75" />
            <circle cx="21.7" cy="21" r="0.75" />
            <animateTransform
              attributeName="transform"
              type="rotate"
              from="0 18 21"
              to="360 18 21"
              dur="5s"
              repeatCount="indefinite"
            />
          </g>

          {/* Right reel */}
          <g>
            <circle cx="42" cy="21" r="5" />
            <circle cx="42" cy="21" r="1.3" fill="currentColor" stroke="none" />
            <circle cx="42" cy="17.3" r="0.75" />
            <circle cx="42" cy="24.7" r="0.75" />
            <circle cx="38.3" cy="21" r="0.75" />
            <circle cx="45.7" cy="21" r="0.75" />
            <animateTransform
              attributeName="transform"
              type="rotate"
              from="0 42 21"
              to="360 42 21"
              dur="5s"
              repeatCount="indefinite"
            />
          </g>
        </svg>
      ) : (
        <svg
          aria-hidden="true"
          className="float-shape shape-gentle pointer-events-none absolute -top-10 right-2 text-amber-600/60 dark:text-amber-200/70 sm:-top-8 sm:right-6"
          style={{ width: 140, height: 90, overflow: "visible" }}
          viewBox="0 0 44 28"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.1"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <g transform="rotate(-14 22 14)">
            <rect x="3" y="4" width="38" height="20" rx="1.5" />
            <rect x="3" y="7.5" width="38" height="13" />
            <rect className="film-frame film-frame-1" x="3.5" y="8" width="12.5" height="12" />
            <rect className="film-frame film-frame-2" x="16" y="8" width="12" height="12" />
            <rect className="film-frame film-frame-3" x="28" y="8" width="12.5" height="12" />
            <rect x="6" y="5" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="10.5" y="5" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="15" y="5" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="19.5" y="5" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="24" y="5" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="28.5" y="5" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="33" y="5" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="6" y="21.4" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="10.5" y="21.4" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="15" y="21.4" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="19.5" y="21.4" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="24" y="21.4" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="28.5" y="21.4" width="2" height="1.6" fill="currentColor" stroke="none" />
            <rect x="33" y="21.4" width="2" height="1.6" fill="currentColor" stroke="none" />
            <line x1="16" y1="7.5" x2="16" y2="20.4" />
            <line x1="28" y1="7.5" x2="28" y2="20.4" />
          </g>
        </svg>
      )}

      <header className="space-y-2 animate-fade-up">
        <p className="text-xs uppercase tracking-[0.3em] text-accent">
          Compose
        </p>
        <h1 className="text-3xl font-bold text-primary">
          {isVideo ? "New video" : "New photo post"}
        </h1>
        <p className="text-sm text-muted">
          {isVideo
            ? "Share a short clip · MP4, WebM or MOV · max 100MB"
            : "Up to 12 photos · auto-converted to AVIF · drag to reorder"}
        </p>
      </header>

      <form
        onSubmit={submit}
        className="frosted-card animate-fade-up space-y-6 p-8"
        style={{ animationDelay: "60ms" }}
      >
        <ImageDropzone items={items} onChange={setItems} disabled={loading} mode={mode} />

        {items.length > 0 && (
          <div className="space-y-2 animate-fade-up">
            <label className="flex items-center justify-between text-xs uppercase tracking-wider text-muted">
              <span>Caption</span>
              <span className={captionLen > captionMax ? "text-red-400" : ""}>
                {captionLen}/{captionMax}
              </span>
            </label>
            <textarea
              value={caption}
              onChange={(e) => setCaption(e.target.value)}
              disabled={loading}
              maxLength={captionMax}
              rows={4}
              placeholder="Write a caption... use #hashtags to tag"
              className="frosted-input text-sm disabled:opacity-50"
            />
          </div>
        )}

        {loading && (
          <div className="space-y-2 animate-fade-in">
            <div className="flex items-center justify-between text-xs text-muted">
              <span>
                {progress < 100
                  ? "Uploading..."
                  : isVideo
                  ? "Saving video..."
                  : "Converting to AVIF..."}
              </span>
              <span>{progress}%</span>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-white/10">
              <div
                className="h-full bg-accent transition-all duration-300"
                style={{ width: `${progress}%` }}
              />
            </div>
          </div>
        )}

        {error && (
          <p className="animate-fade-in rounded-2xl border border-red-400/30 bg-red-400/10 px-4 py-3 text-sm text-red-300">
            {error}
          </p>
        )}

        <div className="flex items-center justify-between gap-3">
          <button
            type="button"
            onClick={() => navigate("/")}
            disabled={loading}
            className="unfrost rounded-full border border-surface-border bg-white/5 px-5 py-2.5 text-sm text-primary transition hover:bg-white/10 disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={loading || items.length === 0}
            aria-label={loading ? "Posting" : "Post"}
            className="unfrost flex h-11 w-11 items-center justify-center rounded-full bg-accent text-accent-text shadow-lg shadow-slate-900/20 transition hover:bg-amber-300 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {loading ? (
              <span className="inline-block h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
            ) : (
              <PaperPlaneIcon className="h-5 w-5 -translate-x-0.5 translate-y-0.5" />
            )}
          </button>
        </div>
      </form>
    </div>
  );
};

export default Posts;
