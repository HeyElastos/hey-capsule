import { useEffect, useRef, useState } from "react";
import {
  Link,
  Navigate,
  Route,
  Routes,
  useLocation,
  useNavigate,
} from "react-router-dom";
import Home from "./pages/Home";
import Profile from "./pages/Profile";
import Videos from "./pages/Clips";
import SignUp from "./pages/SignUp";
import Posts from "./pages/Posts";
import PostDetail from "./pages/PostDetail";
import VideoPlayer from "./pages/VideoPlayer";
import FloatingDock from "./components/FloatingDock";
import SignInModal from "./components/SignInModal";
import {
  CameraIcon,
  VideoIcon,
  LogoutIcon,
} from "./components/icons";

const ROUTE_ORDER = { "/": 0, "/videos": 1 };
const PATH_TO_MODE = { "/": "photo", "/videos": "video" };

const App = () => {
  const user = JSON.parse(localStorage.getItem("profile") || "null");
  const navigate = useNavigate();
  const location = useLocation();
  const prevPathRef = useRef(location.pathname);
  const [direction, setDirection] = useState(null);
  const [mode, setMode] = useState(() => localStorage.getItem("mode") || "photo");
  const [signinOpen, setSigninOpen] = useState(false);

  useEffect(() => {
    const openHandler = () => setSigninOpen(true);
    window.addEventListener("open-signin", openHandler);
    return () => window.removeEventListener("open-signin", openHandler);
  }, []);

  useEffect(() => {
    document.documentElement.classList.add("dark");
    document.documentElement.classList.remove("light");
    localStorage.setItem("theme", "dark");
  }, []);

  useEffect(() => {
    const prev = ROUTE_ORDER[prevPathRef.current];
    const curr = ROUTE_ORDER[location.pathname];
    if (prev !== undefined && curr !== undefined && prev !== curr) {
      setDirection(curr > prev ? "right" : "left");
    } else {
      setDirection(null);
    }
    prevPathRef.current = location.pathname;

    const nextMode = PATH_TO_MODE[location.pathname];
    if (nextMode && nextMode !== mode) {
      setMode(nextMode);
      localStorage.setItem("mode", nextMode);
      window.dispatchEvent(new CustomEvent("modechange", { detail: nextMode }));
    }
  }, [location.pathname, mode]);

  const handleLogout = () => {
    localStorage.removeItem("profile");
    navigate("/");
    window.location.reload();
  };

  const isOnProfile = location.pathname.startsWith("/profile");

  const setModeInPlace = (next) => (event) => {
    if (!isOnProfile) return;
    event.preventDefault();
    setMode(next);
    localStorage.setItem("mode", next);
    window.dispatchEvent(new CustomEvent("modechange", { detail: next }));
  };

  return (
    <div className="min-h-screen text-primary">
      {user && (
        <header className="sticky top-0 z-30 bg-surface-soft/95 backdrop-blur-xl shadow-[0_16px_40px_-18px_rgba(0,0,0,0.15)]">
          <div className="mx-auto flex max-w-6xl items-center justify-between px-4 py-3 sm:px-6">
            <Link
              to="/"
              className="text-3xl font-semibold text-primary logo-handwritten sm:text-5xl"
            >
              Hey
            </Link>

            <nav className="flex flex-1 items-center justify-center gap-8 text-sm sm:gap-12">
              <Link
                to="/"
                onClick={setModeInPlace("photo")}
                className={`icon-btn tab-icon ${mode === "photo" ? "is-active" : ""}`}
                aria-label="Photos"
                aria-current={mode === "photo" ? "page" : undefined}
              >
                <CameraIcon className="h-6 w-6" />
              </Link>
              <Link
                to="/videos"
                onClick={setModeInPlace("video")}
                className={`icon-btn tab-icon ${mode === "video" ? "is-active" : ""}`}
                aria-label="Videos"
                aria-current={mode === "video" ? "page" : undefined}
              >
                <VideoIcon className="h-6 w-6" />
              </Link>
            </nav>

            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={handleLogout}
                className="icon-btn"
                aria-label="Log out"
              >
                <LogoutIcon className="h-5 w-5" />
              </button>
            </div>
          </div>
        </header>
      )}

      <main className="mx-auto max-w-6xl px-4 py-10 sm:px-6">
        {user && <FloatingDock onClose={() => {}} />}
        <div
          key={direction ? location.pathname : "static"}
          className={direction ? `route-switch route-switch-${direction}` : ""}
        >
          <Routes location={location}>
            <Route path="/" element={<Home />} />
            <Route path="/profile" element={<Profile />} />
            <Route path="/profile/:userId" element={<Profile />} />
            <Route path="/videos" element={<Videos />} />
            <Route path="/clips" element={<Navigate to="/videos" replace />} />
            <Route path="/signup" element={<SignUp />} />
            <Route path="/posts" element={<Posts />} />
            <Route path="/p/:id" element={<PostDetail />} />
            <Route path="/v/:id" element={<VideoPlayer />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </div>
      </main>

      {signinOpen && (
        <SignInModal
          onClose={() => setSigninOpen(false)}
          onSuccess={() => {
            setSigninOpen(false);
            navigate("/");
          }}
        />
      )}
    </div>
  );
};

export default App;
