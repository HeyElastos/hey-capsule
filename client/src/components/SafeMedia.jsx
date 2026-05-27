import { useEffect, useState } from "react";

// Translate elastos://<cid>[/path] URLs to a runtime-gateway URL that the
// browser can actually fetch. Capsule mode posts media to IPFS and stores
// the result as `elastos://<cid>` so the value is portable across nodes —
// but <img src=elastos://…> wouldn't load. The runtime exposes content
// under /api/localhost/WebSpaces/Elastos/content/<cid> as a real HTTP
// resource. Server-mode URLs (/uploads/...) and absolute http(s) pass
// through unchanged.
// Match lib/runtime.js so <img src> URLs include the YunoHost subpath.
const API_BASE = (() => {
  if (typeof window === "undefined") return "";
  const m = window.location.pathname.match(/^(.*?)\/apps\/[^/]+\//);
  return m ? m[1] : "";
})();

const resolveMediaSrc = (src) => {
  if (typeof src !== "string" || !src.startsWith("elastos://")) return src;
  const rest = src.slice("elastos://".length);
  const [cid, ...path] = rest.split("/");
  const suffix = path.length ? `/${path.join("/")}` : "";
  return `${API_BASE}/api/localhost/WebSpaces/Elastos/content/${encodeURIComponent(cid)}${suffix}`;
};

// Image that swaps to `fallback` if the source fails to load — including the
// tricky case where the browser served a broken image from cache and marked
// `complete: true` before our onError handler could attach. Reset on src change.
export const SafeImage = ({ src, fallback = null, onError, ...rest }) => {
  const resolved = resolveMediaSrc(src);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
  }, [src]);

  const handleRef = (node) => {
    if (!node) return;
    if (node.complete && node.naturalWidth === 0 && node.src) {
      setFailed(true);
    }
  };

  const handleError = (e) => {
    setFailed(true);
    onError?.(e);
  };

  if (!resolved || failed) return fallback;
  return <img ref={handleRef} src={resolved} onError={handleError} {...rest} />;
};

// Same idea for <video>: fall back if the source fails. Useful for thumbnail
// previews on profile/clip grids where a 404'd video would otherwise show an
// ugly "video unavailable" native UI.
export const SafeVideo = ({ src, fallback = null, onError, ...rest }) => {
  const [failed, setFailed] = useState(false);
  const resolved = resolveMediaSrc(src);

  useEffect(() => {
    setFailed(false);
  }, [src]);

  const handleRef = (node) => {
    if (!node) return;
    if (node.error) setFailed(true);
  };

  const handleError = (e) => {
    setFailed(true);
    onError?.(e);
  };

  if (!resolved || failed) return fallback;
  return <video ref={handleRef} src={resolved} onError={handleError} {...rest} />;
};

// Exported in case other components want to resolve URLs themselves
// (e.g. background-image: url(...)).
export { resolveMediaSrc };
