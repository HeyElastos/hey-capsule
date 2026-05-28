import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { updateProfile } from "../api/auth";
import { CameraIcon } from "../components/icons";
import { SafeImage } from "../components/SafeMedia";
import { useProfile, setProfile as saveProfile } from "../hooks/useProfile";
import { FloatingScene, HeyMark } from "./Landing";

const BIO_MAX = 280;

const NextArrow = () => (
  <svg
    viewBox="0 0 24 24"
    className="h-4 w-4 fill-none stroke-current stroke-[2] transition-transform duration-200 group-hover:translate-x-1"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M5 12h14M13 5l7 7-7 7" />
  </svg>
);

const Onboarding = () => {
  const navigate = useNavigate();
  const profile = useProfile();
  const token = profile?.accessToken;
  const [step, setStep] = useState(1);

  const fileInputRef = useRef(null);
  const [avatarFile, setAvatarFile] = useState(null);
  const [avatarPreview, setAvatarPreview] = useState(profile?.user?.avatar || "");
  const [bio, setBio] = useState(profile?.user?.bio || "");
  const [nickname, setNickname] = useState(profile?.user?.name || "");
  const [didCopied, setDidCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  const didKey = profile?.user?.didKey || "";
  const handleCopyDid = async () => {
    if (!didKey) return;
    try {
      await navigator.clipboard.writeText(didKey);
      setDidCopied(true);
      setTimeout(() => setDidCopied(false), 1500);
    } catch (_) { /* ignore */ }
  };

  // If somehow we land here without a session, send them home.
  useEffect(() => {
    if (!profile) navigate("/");
  }, [profile, navigate]);

  // Clean up object URLs when the preview changes / unmounts
  useEffect(() => {
    return () => {
      if (avatarPreview && avatarPreview.startsWith("blob:")) {
        URL.revokeObjectURL(avatarPreview);
      }
    };
  }, [avatarPreview]);

  if (!profile) return null;

  const displayName = profile?.user?.name || "friend";
  const initials = (displayName || "?").slice(0, 2).toUpperCase();

  const handlePickAvatar = () => fileInputRef.current?.click();

  const handleAvatarChange = (event) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (!file.type.startsWith("image/")) {
      setError("Photo must be an image.");
      return;
    }
    if (file.size > 10 * 1024 * 1024) {
      setError("Photo is over 10MB.");
      return;
    }
    setError(null);
    setAvatarFile(file);
    setAvatarPreview(URL.createObjectURL(file));
  };

  const finish = async (skipSave) => {
    if (skipSave) {
      setStep(3);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const payload = {};
      if (bio !== (profile?.user?.bio || "")) payload.bio = bio;
      if (avatarFile) payload.avatar = avatarFile;
      const trimmedNick = (nickname || "").trim();
      if (trimmedNick && trimmedNick !== (profile?.user?.name || "")) {
        payload.name = trimmedNick;
      }
      if (Object.keys(payload).length > 0) {
        const data = await updateProfile(payload, token);
        // Mirror updated user fields into the cached profile.
        if (profile) {
          saveProfile({ ...profile, user: { ...profile.user, ...data.user } });
        }
      }
      setStep(3);
    } catch (e) {
      setError(e.response?.data?.message || "Could not save profile.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="relative -mt-10 flex min-h-screen flex-col items-center justify-start px-4 pt-12 pb-10 sm:pt-16">
      <FloatingScene />

      <div className="relative z-10 mx-auto w-full max-w-xl">
        {step === 1 && (
          <div className="text-center animate-fade-up">
            <p
              className="mb-6 text-xs uppercase tracking-[0.4em] text-muted animate-fade-in"
              style={{ animationDelay: "0.3s" }}
            >
              Welcome to
            </p>

            <HeyMark />

            <h1
              className="mt-10 logo-handwritten text-4xl text-primary sm:text-5xl animate-fade-up"
              style={{ animationDelay: "0.8s" }}
            >
              Hi {displayName} 👋
            </h1>

            <p
              className="mx-auto mt-6 max-w-md text-base leading-7 text-muted animate-fade-up"
              style={{ animationDelay: "1.1s" }}
            >
              Hey is your own slice of social — image-first, key-based, and
              built on Elastos. Share photos and short videos with whoever you
              choose, react with any emoji, repost what moves you. No ads,
              no algorithm, no email lists.
            </p>

            <p
              className="mx-auto mt-4 max-w-md text-sm leading-6 text-muted animate-fade-up"
              style={{ animationDelay: "1.3s" }}
            >
              Your passkey is your identity — the same one across every
              Elastos app on this device. Nothing to remember, nothing to
              lose.
            </p>

            <button
              type="button"
              onClick={() => setStep(2)}
              className="unfrost group mt-10 inline-flex items-center justify-center gap-2 rounded-full bg-accent px-8 py-3 text-sm font-semibold text-accent-text shadow-lg shadow-slate-900/20 transition hover:bg-amber-300 animate-fade-up"
              style={{ animationDelay: "1.5s" }}
            >
              Next
              <NextArrow />
            </button>
          </div>
        )}

        {step === 2 && (
          <div className="frosted-card animate-fade-up rounded-3xl p-8 text-left">
            <p className="text-xs uppercase tracking-[0.3em] text-accent">
              Step 2 of 2
            </p>
            <h2 className="mt-2 text-2xl font-bold text-primary">
              Set up your profile
            </h2>
            <p className="mt-1 text-sm text-muted">
              Add a photo and a short bio so people can find you. You can skip
              and do this later.
            </p>

            {/* Avatar */}
            <div className="mt-6 flex items-center gap-4">
              <button
                type="button"
                onClick={handlePickAvatar}
                disabled={busy}
                className="unfrost group relative h-24 w-24 overflow-hidden rounded-full shadow-2xl shadow-slate-900/30 ring-4 ring-white/10 transition hover:ring-white/30 disabled:opacity-70"
                aria-label="Upload avatar"
              >
                <SafeImage
                  src={avatarPreview}
                  alt=""
                  fallback={
                    <div className="absolute inset-0 flex items-center justify-center bg-gradient-to-br from-amber-300 to-amber-600 text-3xl font-black text-slate-900">
                      {initials}
                    </div>
                  }
                  className="absolute inset-0 h-full w-full object-cover"
                />
                <div className="absolute inset-0 flex items-center justify-center bg-black/55 opacity-0 transition-opacity duration-200 group-hover:opacity-100">
                  <CameraIcon className="h-6 w-6 text-white" />
                </div>
              </button>

              <div className="flex-1">
                <p className="text-sm font-medium text-primary">Profile photo</p>
                <p className="text-xs text-muted">
                  Click the circle to upload. Square works best.
                </p>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*"
                  onChange={handleAvatarChange}
                  className="hidden"
                />
              </div>
            </div>

            {/* Nickname */}
            <div className="mt-6 space-y-1.5">
              <label className="text-xs uppercase tracking-wider text-muted">
                Nickname
              </label>
              <input
                type="text"
                value={nickname}
                onChange={(e) => setNickname(e.target.value)}
                disabled={busy}
                maxLength={30}
                placeholder="What should friends call you?"
                className="frosted-input w-full text-sm disabled:opacity-50"
              />
            </div>

            {/* Bio */}
            <div className="mt-4 space-y-1.5">
              <label className="flex items-center justify-between text-xs uppercase tracking-wider text-muted">
                <span>Bio</span>
                <span
                  className={
                    bio.length > BIO_MAX ? "text-red-500 dark:text-red-400" : ""
                  }
                >
                  {bio.length}/{BIO_MAX}
                </span>
              </label>
              <textarea
                value={bio}
                onChange={(e) => setBio(e.target.value)}
                disabled={busy}
                maxLength={BIO_MAX}
                rows={3}
                placeholder="Say a few words about yourself..."
                className="frosted-input w-full text-sm disabled:opacity-50"
              />
            </div>

            {/* DID share box — symmetric to Hey Chat's add-contact UX. */}
            {didKey && (
              <div className="mt-4 rounded-xl border border-surface-border bg-white/5 p-3">
                <div className="text-[11px] font-medium uppercase tracking-wider text-muted">
                  Your DID — share this with friends so they can find you
                </div>
                <div className="mt-1.5 flex items-center gap-2">
                  <code className="flex-1 font-mono text-[11px] text-primary/90 break-all">
                    {didKey}
                  </code>
                  <button
                    type="button"
                    onClick={handleCopyDid}
                    className="shrink-0 rounded-md bg-white/10 px-2.5 py-1 text-[11px] font-semibold text-primary hover:bg-accent hover:text-accent-text transition-colors"
                  >
                    {didCopied ? "Copied ✓" : "Copy"}
                  </button>
                </div>
              </div>
            )}

            {error && (
              <p className="mt-3 text-sm text-red-500 dark:text-red-400">
                {error}
              </p>
            )}

            <div className="mt-8 flex items-center justify-between gap-3">
              <button
                type="button"
                onClick={() => setStep(1)}
                disabled={busy}
                className="unfrost text-sm text-muted transition hover:text-primary disabled:opacity-50"
              >
                ← Back
              </button>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => finish(true)}
                  disabled={busy}
                  className="unfrost rounded-full border border-surface-border bg-white/5 px-5 py-2 text-sm font-medium text-primary transition hover:bg-white/10 disabled:opacity-50"
                >
                  Skip for now
                </button>
                <button
                  type="button"
                  onClick={() => finish(false)}
                  disabled={busy || bio.length > BIO_MAX}
                  style={{ backgroundColor: "rgb(34 197 94)" }}
                  className="group inline-flex items-center gap-2 rounded-full border-2 border-green-600 px-6 py-2.5 text-sm font-semibold text-white shadow-md shadow-green-900/30 transition hover:!bg-green-600 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {busy ? "Saving..." : "Finish"}
                  {!busy && <NextArrow />}
                </button>
              </div>
            </div>
          </div>
        )}

        {step === 3 && (
          <div className="relative text-center animate-fade-up">
            {/* Floating confetti pieces */}
            <span aria-hidden="true" className="pointer-events-none absolute -top-6 left-8 text-2xl confetti-1">🎊</span>
            <span aria-hidden="true" className="pointer-events-none absolute top-2 right-6 text-2xl confetti-2">✨</span>
            <span aria-hidden="true" className="pointer-events-none absolute -top-2 right-1/3 text-xl confetti-3">🎈</span>
            <span aria-hidden="true" className="pointer-events-none absolute top-12 -left-2 text-xl confetti-2">⭐</span>
            <span aria-hidden="true" className="pointer-events-none absolute top-20 right-2 text-lg confetti-1">💫</span>

            <div className="inline-block popper-shake">
              <span role="img" aria-label="party popper" className="text-7xl sm:text-8xl drop-shadow-xl">
                🎉
              </span>
            </div>

            <h1
              className="mt-4 logo-handwritten text-4xl text-primary sm:text-5xl animate-fade-up"
              style={{ animationDelay: "0.2s" }}
            >
              You're all set!
            </h1>

            <p
              className="mx-auto mt-5 max-w-md text-base leading-7 text-muted animate-fade-up"
              style={{ animationDelay: "0.4s" }}
            >
              Welcome to Hey, <span className="text-primary font-semibold">{displayName}</span>.
              Your profile is ready and your feed is waiting. Time to dig in,
              share your first photo or clip, and meet the rest of the crew.
            </p>

            <button
              type="button"
              onClick={() => navigate("/")}
              style={{ backgroundColor: "rgb(34 197 94)" }}
              className="group mt-10 inline-flex items-center justify-center gap-2 rounded-full border-2 border-green-600 px-8 py-3 text-sm font-semibold text-white shadow-md shadow-green-900/30 transition hover:!bg-green-600 animate-fade-up"
            >
              Let's go
              <NextArrow />
            </button>
          </div>
        )}
      </div>
    </div>
  );
};

export default Onboarding;
