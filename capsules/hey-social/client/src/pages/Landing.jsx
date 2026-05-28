import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { signInViaRuntime, passkeySupported } from "../api/passkey";
import { signUp } from "../api/auth";
import { setProfile } from "../hooks/useProfile";
import { copyToClipboard } from "../utils/clipboard";

export const FloatingScene = () => (
  <div className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
    {/* Soft gradient glow blobs — closest-side keeps the colored area well inside
        the box, so the box edge stays fully transparent and nothing visible
        gets clipped by the parent's overflow-hidden. */}
    <div
      className="float-shape glow"
      style={{
        top: "6%",
        left: "8%",
        width: "420px",
        height: "420px",
        background:
          "radial-gradient(circle closest-side at center, rgba(212,184,75,0.75) 0%, rgba(212,184,75,0.30) 40%, transparent 75%)",
        filter: "blur(80px)",
      }}
    />
    <div
      className="float-shape glow"
      style={{
        bottom: "8%",
        right: "8%",
        width: "520px",
        height: "520px",
        background:
          "radial-gradient(circle closest-side at center, rgba(96,165,250,0.60) 0%, rgba(96,165,250,0.22) 40%, transparent 75%)",
        filter: "blur(90px)",
        animationDelay: "1.5s",
      }}
    />
    <div
      className="float-shape glow"
      style={{
        top: "38%",
        right: "26%",
        width: "320px",
        height: "320px",
        background:
          "radial-gradient(circle closest-side at center, rgba(244,114,182,0.50) 0%, rgba(244,114,182,0.18) 40%, transparent 75%)",
        filter: "blur(70px)",
        animationDelay: "3s",
      }}
    />

    {/* Outline circle */}
    <svg
      className="float-shape shape-a text-amber-700/40 dark:text-accent/60"
      style={{ top: "14%", right: "16%", width: 80, height: 80 }}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1"
    >
      <circle cx="12" cy="12" r="10" />
    </svg>

    {/* Triangle */}
    <svg
      className="float-shape shape-b text-sky-700/45 dark:text-sky-300/70"
      style={{ top: "22%", left: "18%", width: 70, height: 70 }}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinejoin="round"
    >
      <path d="M12 3 21 20H3z" />
    </svg>

    {/* Plus */}
    <svg
      className="float-shape shape-c text-pink-600/50 dark:text-pink-300/70"
      style={{ bottom: "26%", left: "12%", width: 56, height: 56 }}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
    >
      <path d="M12 5v14M5 12h14" />
    </svg>

    {/* Sparkle / sun above the "y" in Hey */}
    <svg
      className="float-shape shape-d text-amber-600/70 dark:text-amber-200/80"
      style={{ top: "20%", left: "58%", width: 64, height: 64 }}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinecap="round"
    >
      <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.5 5.5l2.8 2.8M15.7 15.7l2.8 2.8M5.5 18.5l2.8-2.8M15.7 8.3l2.8-2.8" />
    </svg>

    {/* Square outline */}
    <div
      className="float-shape shape-c"
      style={{ top: "62%", right: "8%", width: 60, height: 60, animationDelay: "0.7s" }}
    >
      <svg
        className="square-tick text-emerald-700/40 dark:text-emerald-300/60"
        style={{ width: "100%", height: "100%" }}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.25"
      >
        <rect x="3" y="3" width="18" height="18" rx="3" />
      </svg>
    </div>

    {/* Wavy line */}
    <svg
      className="float-shape shape-d text-pink-500/45 dark:text-pink-200/60"
      style={{ top: "70%", left: "22%", width: 100, height: 30, animationDelay: "2.5s" }}
      viewBox="0 0 100 30"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
    >
      <path d="M2 15 Q15 2, 28 15 T54 15 T80 15 T98 15">
        <animate
          attributeName="d"
          values="
            M2 15 Q15 2, 28 15 T54 15 T80 15 T98 15;
            M2 15 Q15 28, 28 15 T54 15 T80 15 T98 15;
            M2 15 Q15 2, 28 15 T54 15 T80 15 T98 15
          "
          dur="6s"
          repeatCount="indefinite"
          calcMode="spline"
          keyTimes="0; 0.5; 1"
          keySplines="0.42 0 0.58 1; 0.42 0 0.58 1"
        />
      </path>
    </svg>
  </div>
);

export const HeyMark = () => (
  <div className="relative inline-block pb-8">
    <svg
      className="hey-underline absolute left-1/2 -translate-x-1/2 -z-10"
      style={{ bottom: "22%", width: "88%", opacity: 0.85 }}
      viewBox="0 0 240 30"
      fill="none"
      stroke="currentColor"
      strokeWidth="5"
      strokeLinecap="round"
    >
      <path d="M8 18 Q60 4, 120 14 T232 12" className="text-accent" />
    </svg>

    <svg
      viewBox="0 0 480 280"
      className="hey-wordmark relative block mx-auto w-[280px] sm:w-[420px]"
      aria-label="Hey"
    >
      <defs>
        {[
          { ch: "H", x: 110 },
          { ch: "e", x: 230 },
          { ch: "y", x: 320 },
        ].map(({ ch, x }, i) => (
          <mask id={`hey-mask-${i}`} key={ch}>
            <text
              x={x}
              y={200}
              className="hey-pencil"
              style={{
                fontFamily: "'Dancing Script', cursive",
                fontWeight: 600,
                fontSize: "200px",
                animationDelay: `${i * 0.9}s`,
              }}
            >
              {ch}
            </text>
          </mask>
        ))}
      </defs>

      {[
        { ch: "H", x: 110 },
        { ch: "e", x: 230 },
        { ch: "y", x: 320 },
      ].map(({ ch, x }, i) => (
        <text
          key={ch}
          x={x}
          y={200}
          className="hey-fill"
          mask={`url(#hey-mask-${i})`}
          style={{
            fontFamily: "'Dancing Script', cursive",
            fontWeight: 600,
            fontSize: "200px",
          }}
        >
          {ch}
        </text>
      ))}
    </svg>
  </div>
);

const ArrowCue = () => (
  <div className="absolute -top-5 right-0 hidden sm:block">
    <span className="caret-cue inline-block rounded-full bg-accent px-3 py-1 text-xs font-bold uppercase tracking-wider text-accent-text shadow-lg">
      Start here
    </span>
  </div>
);

const Landing = () => {
  const navigate = useNavigate();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const canUsePasskey = passkeySupported();

  // Recovery-key fallback state (secondary path for users without a
  // passkey-capable authenticator, or who explicitly prefer the
  // write-down-a-string model).
  const [showRecovery, setShowRecovery] = useState(false);
  const [recoveryName, setRecoveryName] = useState("");
  const [generatedKey, setGeneratedKey] = useState(null);
  const [pendingProfile, setPendingProfile] = useState(null);
  const [keyCopied, setKeyCopied] = useState(false);

  const handlePasskey = async () => {
    setError(null);
    setBusy(true);
    try {
      const data = await signInViaRuntime();
      setProfile({
        user: data.user,
        accessToken: data.accessToken,
        refreshToken: data.refreshToken,
      });
      navigate("/welcome");
    } catch (err) {
      const msg = err?.message || "Passkey sign-in failed.";
      // User-cancel: the WebAuthn API throws NotAllowedError. Don't
      // surface a scary message; just let them try again.
      if (/NotAllowedError|AbortError|cancelled|canceled/i.test(msg)) {
        setError("Passkey prompt closed. Tap to try again.");
      } else {
        setError(msg);
      }
    } finally {
      setBusy(false);
    }
  };

  const handleGenerateKey = async () => {
    setError(null);
    if (!recoveryName.trim()) {
      setError("Pick a nickname for the recovery-key path.");
      return;
    }
    setBusy(true);
    try {
      const data = await signUp({ name: recoveryName.trim() });
      setPendingProfile({
        user: data.user,
        accessToken: data.accessToken,
        refreshToken: data.refreshToken,
      });
      setGeneratedKey(data.authKey);
    } catch (err) {
      setError(err?.response?.data?.message || err?.message || "Could not generate key.");
    } finally {
      setBusy(false);
    }
  };

  const handleCopyKey = async () => {
    if (!generatedKey) return;
    const ok = await copyToClipboard(generatedKey);
    if (ok) {
      setKeyCopied(true);
      setTimeout(() => setKeyCopied(false), 1500);
    }
  };

  const handleRecoveryContinue = () => {
    if (pendingProfile) setProfile(pendingProfile);
    navigate("/welcome");
  };

  return (
    <div className="relative -mt-10 flex min-h-[80vh] flex-col items-center justify-center px-4 py-10">
      <FloatingScene />

      <div className="relative z-10 mx-auto max-w-2xl text-center">
        <p
          className="mb-6 text-xs uppercase tracking-[0.4em] text-muted animate-fade-in"
          style={{ animationDelay: "0.6s" }}
        >
          Your own social media on Elastos
        </p>

        <HeyMark />

        <p
          className="mx-auto mt-4 max-w-lg text-base leading-7 text-muted animate-fade-up"
          style={{ animationDelay: "1.0s" }}
        >
          Photo, video, and chat — peer-to-peer over Elastos. Sign in with the same
          passkey you used to set up this device. No password, no recovery key.
        </p>

        <div
          className="relative mx-auto mt-12 max-w-sm animate-fade-up"
          style={{ animationDelay: "1.3s" }}
        >
          {canUsePasskey ? (
            <button
              type="button"
              onClick={handlePasskey}
              disabled={busy}
              className="unfrost group inline-flex w-full items-center justify-center gap-3 rounded-full bg-accent px-8 py-4 text-base font-semibold text-accent-text shadow-xl shadow-slate-900/25 transition hover:bg-amber-300 disabled:cursor-not-allowed disabled:opacity-60"
            >
              <svg viewBox="0 0 24 24" className="h-5 w-5 fill-current">
                <path d="M12 2a5 5 0 0 0-5 5v3H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8a2 2 0 0 0-2-2h-1V7a5 5 0 0 0-5-5Zm-3 8V7a3 3 0 0 1 6 0v3H9Z" />
              </svg>
              {busy ? "Waiting for passkey…" : "Sign in with passkey"}
            </button>
          ) : (
            <div className="frosted-card p-5 text-sm text-muted">
              Your browser doesn't support passkeys. Hey needs a passkey-capable
              browser (modern Chrome / Edge / Safari / Firefox).
            </div>
          )}

          {error && (
            <p className="mt-4 animate-fade-in text-sm text-red-400">{error}</p>
          )}

          <p
            className="mt-8 text-xs text-muted animate-fade-in"
            style={{ animationDelay: "1.6s" }}
          >
            One tap. Same passkey as System. Nothing to remember.
          </p>

          {/* Recovery-key fallback — for users without a passkey-capable
              authenticator, or who explicitly want the write-it-down model. */}
          {!generatedKey && (
            <div className="mt-8 border-t border-surface-border pt-6 text-left animate-fade-in">
              {!showRecovery ? (
                <button
                  type="button"
                  onClick={() => setShowRecovery(true)}
                  className="unfrost mx-auto block text-xs text-muted underline-offset-4 hover:text-primary hover:underline transition-colors"
                >
                  No passkey? Use a recovery key instead
                </button>
              ) : (
                <div className="frosted-card p-4 space-y-3">
                  <div className="flex items-baseline justify-between">
                    <p className="text-xs uppercase tracking-wider text-muted">
                      Recovery key
                    </p>
                    <button
                      type="button"
                      onClick={() => { setShowRecovery(false); setRecoveryName(""); setError(null); }}
                      className="unfrost text-[11px] text-muted hover:text-primary transition-colors"
                    >
                      Hide
                    </button>
                  </div>
                  <p className="text-[12px] leading-relaxed text-muted">
                    We'll generate a 32-byte secret you must save somewhere safe. Lose it,
                    lose your account.
                  </p>
                  <input
                    type="text"
                    value={recoveryName}
                    onChange={(e) => setRecoveryName(e.target.value)}
                    disabled={busy}
                    maxLength={30}
                    placeholder="Pick a nickname"
                    className="unfrost w-full rounded-xl bg-white/5 px-3 py-2 text-sm text-primary outline-none placeholder:text-muted border border-surface-border focus:border-accent"
                  />
                  <button
                    type="button"
                    onClick={handleGenerateKey}
                    disabled={busy || !recoveryName.trim()}
                    className="unfrost w-full rounded-full border border-surface-border bg-white/5 px-4 py-2 text-sm font-semibold text-primary transition hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    {busy ? "Generating…" : "Generate a recovery key"}
                  </button>
                </div>
              )}
            </div>
          )}

          {/* Generated recovery key panel — replaces the form once the
              key exists. User copies + clicks Continue. */}
          {generatedKey && (
            <div className="mt-8 animate-pop-in frosted-card p-5 text-left space-y-4">
              <header className="flex items-center gap-2">
                <span className="inline-flex h-2 w-2 animate-pulse rounded-full bg-emerald-500" />
                <p className="text-xs uppercase tracking-wider text-emerald-600 dark:text-emerald-300">
                  Welcome, {recoveryName.trim()}
                </p>
              </header>
              <p className="text-sm text-muted">
                This is your recovery key.{" "}
                <strong className="text-primary">Save it now</strong> — it's the only way to sign back in.
              </p>
              <p className="select-all break-all rounded-lg bg-black/10 px-3 py-2 text-center font-mono text-xs text-primary/90 dark:bg-white/5">
                {generatedKey}
              </p>
              <button
                type="button"
                onClick={handleCopyKey}
                className="unfrost w-full rounded-full bg-accent px-5 py-2.5 text-sm font-semibold text-accent-text transition hover:bg-amber-300"
              >
                {keyCopied ? "Copied ✓" : "Copy key"}
              </button>
              <button
                type="button"
                onClick={handleRecoveryContinue}
                style={{ backgroundColor: "rgb(34 197 94)" }}
                className="group inline-flex w-full items-center justify-center gap-2 rounded-full border-2 border-green-600 px-5 py-2.5 text-sm font-semibold text-white shadow-md shadow-green-900/30 transition hover:!bg-green-600"
              >
                I saved it — continue
                <svg viewBox="0 0 24 24" className="h-3.5 w-3.5 fill-none stroke-current stroke-[2]" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M5 12h14M13 5l7 7-7 7" />
                </svg>
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default Landing;
