import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useProfile } from "../hooks/useProfile";
import {
  listThreads,
  getThread,
  sendMessage,
  editMessage,
  deleteMessage,
  reactToMessage,
  markThreadRead,
  uploadAttachments,
  uploadVoice,
  listRooms,
  getRoom,
  sendRoomMessage,
} from "../api/chat";
import AddFriendModal from "../components/AddFriendModal";
import CreateRoomModal from "../components/CreateRoomModal";
import ProfilePopover from "../components/ProfilePopover";
import {
  CloseIcon,
  ImageIcon,
  MicIcon,
  PaperPlaneIcon,
  PlayIcon,
  PlusIcon,
  ShieldCheckIcon,
  StopIcon,
  TrashIcon,
} from "../components/icons";

const POLL_MS = 4000;
const EDIT_WINDOW_MS = 15 * 60 * 1000;
const MAX_ATTACHMENTS = 4;
const MAX_FILE_BYTES = 25 * 1024 * 1024;
const DEFAULT_REACTIONS = ["❤️", "🔥", "😂", "😮", "😢", "👏", "💯", "✨"];
const DID_TRUNC = (s) => (s ? `${s.slice(0, 12)}…${s.slice(-6)}` : "");

const AVATAR_PALETTE = [
  ["from-amber-400", "to-pink-400"],
  ["from-indigo-400", "to-cyan-400"],
  ["from-emerald-400", "to-sky-400"],
  ["from-rose-400", "to-orange-400"],
  ["from-violet-400", "to-fuchsia-400"],
  ["from-yellow-400", "to-red-400"],
];

const paletteFor = (did) => {
  if (!did) return AVATAR_PALETTE[0];
  let h = 0;
  for (let i = 0; i < did.length; i++) h = (h * 31 + did.charCodeAt(i)) | 0;
  return AVATAR_PALETTE[Math.abs(h) % AVATAR_PALETTE.length];
};

const Avatar = ({ name, avatar, did, size = "h-10 w-10", textSize = "text-sm" }) => {
  const letter = (name || did?.slice(9, 10) || "?").charAt(0).toUpperCase();
  if (avatar) {
    return <img src={avatar} alt={name} className={`${size} flex-none rounded-full object-cover`} />;
  }
  const [from, to] = paletteFor(did);
  return (
    <div
      className={`${size} grid flex-none place-items-center rounded-full bg-gradient-to-br ${from} ${to} font-bold text-black/80 ${textSize}`}
    >
      {letter}
    </div>
  );
};

const relativeTime = (ts) => {
  const diff = Date.now() - ts;
  if (diff < 60_000) return "now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h`;
  const date = new Date(ts);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const yest = new Date(today);
  yest.setDate(today.getDate() - 1);
  const msgDate = new Date(date);
  msgDate.setHours(0, 0, 0, 0);
  if (msgDate.getTime() === yest.getTime()) return "yest";
  if (Date.now() - msgDate.getTime() < 7 * 86_400_000) {
    return date.toLocaleDateString([], { weekday: "short" });
  }
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
};

const dayLabel = (ts) => {
  const date = new Date(ts);
  date.setHours(0, 0, 0, 0);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const yest = new Date(today);
  yest.setDate(today.getDate() - 1);
  if (date.getTime() === today.getTime()) return "Today";
  if (date.getTime() === yest.getTime()) return "Yesterday";
  return date.toLocaleDateString([], { weekday: "long", month: "short", day: "numeric" });
};

const groupMessages = (messages) => {
  const days = [];
  let currentDay = null;
  let currentCluster = null;
  const CLUSTER_GAP_MS = 5 * 60_000;

  for (const m of messages) {
    const label = dayLabel(m.ts);
    if (!currentDay || currentDay.day !== label) {
      currentDay = { day: label, clusters: [] };
      days.push(currentDay);
      currentCluster = null;
    }
    const prevInCluster = currentCluster?.messages[currentCluster.messages.length - 1];
    const sameSender = currentCluster?.sender_did === m.sender_did;
    const closeInTime = prevInCluster && m.ts - prevInCluster.ts < CLUSTER_GAP_MS;
    if (!sameSender || !closeInTime) {
      currentCluster = { sender_did: m.sender_did, messages: [] };
      currentDay.clusters.push(currentCluster);
    }
    currentCluster.messages.push(m);
  }
  return days;
};

const formatDuration = (ms) => {
  const total = Math.round(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
};

// One thumbnail in a message's attachment grid. Click image → open in a new
// tab (full-size viewer is a separate epic). Videos play inline. Voice
// messages render as a horizontal player chip.
const AttachmentTile = ({ attachment }) => {
  if (attachment.type === "voice") {
    return (
      <div className="col-span-2 flex items-center gap-2 rounded-xl bg-black/[0.04] px-3 py-2 ring-1 ring-black/5 dark:bg-white/[0.06] dark:ring-white/10">
        <audio
          src={attachment.url}
          controls
          preload="metadata"
          className="h-8 flex-1 [&::-webkit-media-controls-panel]:bg-transparent"
          style={{ maxWidth: 240 }}
        />
        {attachment.duration_ms ? (
          <span className="text-[10px] font-mono text-muted">
            {formatDuration(attachment.duration_ms)}
          </span>
        ) : null}
      </div>
    );
  }
  const aspect = attachment.width && attachment.height
    ? `${attachment.width} / ${attachment.height}`
    : "1 / 1";
  if (attachment.type === "video") {
    return (
      <video
        src={attachment.url}
        controls
        preload="metadata"
        className="h-full w-full bg-black object-cover"
        style={{ aspectRatio: aspect }}
      />
    );
  }
  return (
    <a
      href={attachment.url}
      target="_blank"
      rel="noopener noreferrer"
      className="block h-full w-full"
      style={{ aspectRatio: aspect }}
    >
      <img
        src={attachment.url}
        alt=""
        loading="lazy"
        className="h-full w-full object-cover transition hover:scale-[1.02]"
      />
    </a>
  );
};

// Reaction picker — opens on click of the smile button in the hover toolbar.
const ReactionPicker = ({ onPick, onClose }) => {
  const ref = useRef(null);
  useEffect(() => {
    const handler = (e) => {
      if (!ref.current?.contains(e.target)) onClose?.();
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [onClose]);
  return (
    <div
      ref={ref}
      className="animate-pop-in absolute -top-12 left-1/2 z-20 flex -translate-x-1/2 items-center gap-0.5 rounded-full border border-black/10 bg-white/95 px-1.5 py-1 shadow-lg backdrop-blur-md dark:border-white/10 dark:bg-neutral-900/95"
    >
      {DEFAULT_REACTIONS.map((e) => (
        <button
          key={e}
          type="button"
          onClick={() => onPick(e)}
          className="grid h-8 w-8 place-items-center rounded-full text-base transition hover:scale-125 hover:bg-black/5 dark:hover:bg-white/10"
        >
          {e}
        </button>
      ))}
    </div>
  );
};

const Chat = () => {
  const profile = useProfile();
  const token = profile?.accessToken;
  const myDid = profile?.user?.didKey || "";
  const myName = profile?.user?.name || "";

  const [threads, setThreads] = useState([]);
  const [rooms, setRooms] = useState([]);
  // Unified active-conversation selector. type = "dm" → id is peerDid;
  // type = "room" → id is roomId. Exactly one or none is set.
  const [activeConvo, setActiveConvo] = useState(null);
  const [threadData, setThreadData] = useState(null);
  const [roomData, setRoomData] = useState(null);
  const [createRoomOpen, setCreateRoomOpen] = useState(false);
  const [profileOpen, setProfileOpen] = useState(false);
  const [attachMenuOpen, setAttachMenuOpen] = useState(false);
  const activePeerDid = activeConvo?.type === "dm" ? activeConvo.id : null;
  const activeRoomId = activeConvo?.type === "room" ? activeConvo.id : null;
  const [draft, setDraft] = useState("");
  const [replyingTo, setReplyingTo] = useState(null); // message object
  const [editingId, setEditingId] = useState(null);
  const [editDraft, setEditDraft] = useState("");
  const [pickerForId, setPickerForId] = useState(null);
  const [pendingFiles, setPendingFiles] = useState([]); // File[] selected, not yet uploaded
  const [uploadProgress, setUploadProgress] = useState(0); // 0-100 during upload
  // Voice-recording state: "idle" → "recording" → "preview" → (send or cancel)
  const [recState, setRecState] = useState("idle");
  const [recDuration, setRecDuration] = useState(0); // ms
  const [voiceBlob, setVoiceBlob] = useState(null);
  const [sending, setSending] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [error, setError] = useState(null);

  const threadEndRef = useRef(null);
  const composeRef = useRef(null);
  const editRef = useRef(null);
  const fileInputRef = useRef(null);
  const profileButtonRef = useRef(null);
  const lastMessageCountRef = useRef(0);
  const mediaRecorderRef = useRef(null);
  const recordingChunksRef = useRef([]);
  const recordingStartRef = useRef(0);
  const recordingTimerRef = useRef(null);
  const voicePreviewUrlRef = useRef(null);

  // Object URLs created for pendingFiles previews. Revoked when files change
  // or component unmounts to avoid memory leaks.
  const filePreviewUrls = useMemo(
    () => pendingFiles.map((f) => ({ file: f, url: URL.createObjectURL(f) })),
    [pendingFiles]
  );
  useEffect(() => {
    return () => filePreviewUrls.forEach((p) => URL.revokeObjectURL(p.url));
  }, [filePreviewUrls]);

  // The "current conversation data" — unified view over DMs and rooms.
  const convoData = activePeerDid ? threadData : (activeRoomId ? roomData : null);
  const convoMessages = convoData?.messages || [];

  // Quick lookup of any message in the current convo (for reply previews).
  const messagesById = useMemo(() => {
    const map = new Map();
    for (const m of convoMessages) map.set(m.id, m);
    return map;
  }, [convoMessages]);

  const refreshThreads = useCallback(async () => {
    if (!token) return;
    try {
      const list = await listThreads(token);
      setThreads(list);
    } catch (e) {
      setError(e.response?.data?.message || "Failed to load chats");
    }
  }, [token]);

  const refreshRooms = useCallback(async () => {
    if (!token) return;
    try {
      const list = await listRooms(token);
      setRooms(list);
    } catch (e) {
      setError(e.response?.data?.message || "Failed to load rooms");
    }
  }, [token]);

  const refreshThread = useCallback(async (peerDid) => {
    if (!token || !peerDid) return;
    try {
      const data = await getThread(token, peerDid);
      setThreadData(data);
    } catch (e) {
      setError(e.response?.data?.message || "Failed to load conversation");
    }
  }, [token]);

  const refreshRoom = useCallback(async (roomId) => {
    if (!token || !roomId) return;
    try {
      const data = await getRoom(token, roomId);
      setRoomData(data);
    } catch (e) {
      setError(e.response?.data?.message || "Failed to load room");
    }
  }, [token]);

  useEffect(() => { refreshThreads(); refreshRooms(); }, [refreshThreads, refreshRooms]);

  useEffect(() => {
    if (!token) return;
    const id = setInterval(() => { refreshThreads(); refreshRooms(); }, POLL_MS);
    return () => clearInterval(id);
  }, [token, refreshThreads, refreshRooms]);

  useEffect(() => {
    if (activePeerDid) {
      refreshThread(activePeerDid);
      const id = setInterval(() => refreshThread(activePeerDid), POLL_MS);
      return () => clearInterval(id);
    }
    if (activeRoomId) {
      refreshRoom(activeRoomId);
      const id = setInterval(() => refreshRoom(activeRoomId), POLL_MS);
      return () => clearInterval(id);
    }
  }, [activePeerDid, activeRoomId, refreshThread, refreshRoom]);

  // Mark inbound messages as read whenever we view a thread that has unread
  // ones. Throttled implicitly — we only call when the count of unread
  // changes, so the server call is rare.
  useEffect(() => {
    if (!activePeerDid || !threadData?.messages) return;
    const unreadFromPeer = threadData.messages.some(
      (m) => m.sender_did !== myDid && !m.read_at && !m.deleted_at
    );
    if (unreadFromPeer) {
      markThreadRead(token, activePeerDid).catch(() => {});
    }
  }, [activePeerDid, threadData?.messages, myDid, token]);

  useEffect(() => {
    const count = convoMessages.length || 0;
    if (count > lastMessageCountRef.current) {
      threadEndRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
    }
    lastMessageCountRef.current = count;
  }, [convoMessages.length]);

  useEffect(() => { lastMessageCountRef.current = 0; }, [activePeerDid, activeRoomId]);

  useEffect(() => {
    const el = composeRef.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 140)}px`;
  }, [draft]);

  // Focus the inline edit textarea on open, place cursor at end.
  useEffect(() => {
    if (editingId && editRef.current) {
      editRef.current.focus();
      const len = editRef.current.value.length;
      editRef.current.setSelectionRange(len, len);
    }
  }, [editingId]);

  // ─── Voice recording ─────────────────────────────────────────────
  const cleanupRecording = () => {
    clearInterval(recordingTimerRef.current);
    recordingTimerRef.current = null;
    const recorder = mediaRecorderRef.current;
    if (recorder) {
      try {
        recorder.stream?.getTracks().forEach((t) => t.stop());
      } catch { /* already stopped */ }
    }
    mediaRecorderRef.current = null;
    recordingChunksRef.current = [];
  };

  const cleanupPreview = () => {
    if (voicePreviewUrlRef.current) {
      URL.revokeObjectURL(voicePreviewUrlRef.current);
      voicePreviewUrlRef.current = null;
    }
    setVoiceBlob(null);
    setRecDuration(0);
  };

  // Stop any in-flight recording / preview on unmount.
  useEffect(() => () => {
    cleanupRecording();
    cleanupPreview();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleStartRecording = async () => {
    if (recState !== "idle" || sending) return;
    if (!navigator.mediaDevices?.getUserMedia) {
      setError("Voice recording isn't supported in this browser.");
      return;
    }
    setError(null);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const mime = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
        ? "audio/webm;codecs=opus"
        : (MediaRecorder.isTypeSupported("audio/webm") ? "audio/webm" : "");
      const recorder = mime
        ? new MediaRecorder(stream, { mimeType: mime })
        : new MediaRecorder(stream);

      recordingChunksRef.current = [];
      recorder.ondataavailable = (e) => {
        if (e.data && e.data.size > 0) recordingChunksRef.current.push(e.data);
      };
      recorder.onstop = () => {
        const chunks = recordingChunksRef.current;
        const type = recorder.mimeType?.split(";")[0] || "audio/webm";
        const blob = new Blob(chunks, { type });
        const duration = Date.now() - recordingStartRef.current;
        cleanupRecording();
        if (blob.size < 200) {
          // Too short to be a real recording — bail to idle.
          setRecState("idle");
          return;
        }
        voicePreviewUrlRef.current = URL.createObjectURL(blob);
        setVoiceBlob(blob);
        setRecDuration(duration);
        setRecState("preview");
      };

      mediaRecorderRef.current = recorder;
      recordingStartRef.current = Date.now();
      recorder.start();
      setRecState("recording");
      setRecDuration(0);
      recordingTimerRef.current = setInterval(() => {
        const elapsed = Date.now() - recordingStartRef.current;
        setRecDuration(elapsed);
        if (elapsed > 5 * 60_000) handleStopRecording(); // 5 min hard cap
      }, 100);
    } catch (e) {
      setError(e?.name === "NotAllowedError"
        ? "Microphone access denied."
        : "Couldn't start recording.");
      cleanupRecording();
      setRecState("idle");
    }
  };

  const handleStopRecording = () => {
    const recorder = mediaRecorderRef.current;
    if (!recorder) {
      setRecState("idle");
      return;
    }
    if (recorder.state !== "inactive") {
      recorder.stop();
    } else {
      cleanupRecording();
      setRecState("idle");
    }
  };

  const handleCancelRecording = () => {
    cleanupRecording();
    cleanupPreview();
    setRecState("idle");
  };

  const handleSendVoice = useCallback(async () => {
    if (!voiceBlob || !(activePeerDid || activeRoomId) || sending) return;
    setSending(true);
    setError(null);
    const blob = voiceBlob;
    const duration = recDuration;
    cleanupPreview();
    setRecState("idle");
    try {
      const attachment = await uploadVoice(token, blob, duration, setUploadProgress);
      let newMsg;
      if (activePeerDid) {
        newMsg = await sendMessage(token, activePeerDid, "", null, [attachment]);
        setThreadData((prev) => prev ? { ...prev, messages: [...prev.messages, newMsg] } : prev);
        refreshThreads();
      } else {
        newMsg = await sendRoomMessage(token, activeRoomId, "", null, [attachment]);
        setRoomData((prev) => prev ? { ...prev, messages: [...prev.messages, newMsg] } : prev);
        refreshRooms();
      }
    } catch (err) {
      setError(err.response?.data?.message || "Failed to send voice");
    } finally {
      setSending(false);
      setUploadProgress(0);
    }
  }, [voiceBlob, recDuration, activePeerDid, activeRoomId, sending, token, refreshThreads, refreshRooms]);

  const handleSend = useCallback(async (e) => {
    e?.preventDefault?.();
    if ((!activePeerDid && !activeRoomId) || sending) return;
    const text = draft.trim();
    const hasAttachments = pendingFiles.length > 0;
    if (!text && !hasAttachments) return;
    setSending(true);
    setError(null);
    const replyTargetId = replyingTo?.id || null;
    const filesToUpload = pendingFiles;
    setDraft("");
    setReplyingTo(null);
    setPendingFiles([]);
    setUploadProgress(0);
    try {
      let attachments = [];
      if (hasAttachments) {
        attachments = await uploadAttachments(token, filesToUpload, setUploadProgress);
      }
      let newMsg;
      if (activePeerDid) {
        newMsg = await sendMessage(token, activePeerDid, text, replyTargetId, attachments);
        setThreadData((prev) => prev
          ? { ...prev, messages: [...prev.messages, newMsg] }
          : prev);
        refreshThreads();
      } else {
        newMsg = await sendRoomMessage(token, activeRoomId, text, replyTargetId, attachments);
        setRoomData((prev) => prev
          ? { ...prev, messages: [...prev.messages, newMsg] }
          : prev);
        refreshRooms();
      }
    } catch (err) {
      setError(err.response?.data?.message || "Failed to send");
      setDraft(text);
      setPendingFiles(filesToUpload);
      if (replyTargetId) setReplyingTo(replyingTo);
    } finally {
      setSending(false);
      setUploadProgress(0);
    }
  }, [activePeerDid, activeRoomId, draft, pendingFiles, replyingTo, sending, token, refreshThreads, refreshRooms]);

  const handleKeyDown = (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
    if (e.key === "Escape" && replyingTo) {
      setReplyingTo(null);
    }
  };

  const handlePickFiles = () => {
    fileInputRef.current?.click();
  };

  const handleFilesChosen = (e) => {
    const chosen = Array.from(e.target.files || []);
    e.target.value = ""; // reset so the same file can be re-picked after removal
    if (chosen.length === 0) return;
    const tooBig = chosen.find((f) => f.size > MAX_FILE_BYTES);
    if (tooBig) {
      setError(`"${tooBig.name}" is over 25 MB`);
      return;
    }
    setPendingFiles((prev) => {
      const merged = [...prev, ...chosen];
      if (merged.length > MAX_ATTACHMENTS) {
        setError(`Max ${MAX_ATTACHMENTS} attachments per message`);
        return merged.slice(0, MAX_ATTACHMENTS);
      }
      return merged;
    });
  };

  const handleRemovePendingFile = (idx) => {
    setPendingFiles((prev) => prev.filter((_, i) => i !== idx));
  };

  const handleAdded = (peer) => {
    setAddOpen(false);
    setActiveConvo({ type: "dm", id: peer.did });
    setThreadData({ peer: { did: peer.did, name: peer.name, avatar: peer.avatar }, messages: [] });
    refreshThreads();
  };

  const handleRoomCreated = (room) => {
    setCreateRoomOpen(false);
    setActiveConvo({ type: "room", id: room.id });
    const members = {};
    for (const did of room.member_dids) {
      members[did] = {
        did,
        name: did === myDid ? myName : `${did.slice(0, 16)}…`,
        avatar: "",
      };
    }
    setRoomData({ room, members, messages: [] });
    refreshRooms();
  };

  const updateMessageInPlace = (updated) => {
    if (activePeerDid) {
      setThreadData((prev) => prev
        ? { ...prev, messages: prev.messages.map((m) => (m.id === updated.id ? updated : m)) }
        : prev);
    } else if (activeRoomId) {
      setRoomData((prev) => prev
        ? { ...prev, messages: prev.messages.map((m) => (m.id === updated.id ? updated : m)) }
        : prev);
    }
  };

  // ─── Message actions ─────────────────────────────────────────────
  const handleReact = async (messageId, emoji) => {
    setPickerForId(null);
    try {
      const updated = await reactToMessage(token, messageId, emoji);
      updateMessageInPlace(updated);
    } catch (err) {
      setError(err.response?.data?.message || "Failed to react");
    }
  };

  const handleReplyClick = (m) => {
    setReplyingTo(m);
    composeRef.current?.focus();
  };

  const handleEditStart = (m) => {
    setEditingId(m.id);
    setEditDraft(m.content || "");
  };

  const handleEditCancel = () => {
    setEditingId(null);
    setEditDraft("");
  };

  const handleEditSave = async () => {
    if (!editingId) return;
    const text = editDraft.trim();
    if (!text) {
      handleEditCancel();
      return;
    }
    try {
      const updated = await editMessage(token, editingId, text);
      updateMessageInPlace(updated);
      handleEditCancel();
    } catch (err) {
      setError(err.response?.data?.message || "Failed to edit");
    }
  };

  const handleDelete = async (messageId) => {
    if (!window.confirm("Delete this message?")) return;
    try {
      const updated = await deleteMessage(token, messageId);
      updateMessageInPlace(updated);
    } catch (err) {
      setError(err.response?.data?.message || "Failed to delete");
    }
  };

  const grouped = useMemo(
    () => groupMessages(convoMessages),
    [convoMessages]
  );

  // For group chats we need per-sender display info on each message.
  const senderInfo = (did) => {
    if (activeRoomId && roomData?.members?.[did]) return roomData.members[did];
    if (did === myDid) return { did, name: myName, avatar: "" };
    if (activePeerDid && threadData?.peer?.did === did) {
      return {
        did,
        name: threadData.peer.name,
        avatar: threadData.peer.avatar,
      };
    }
    return { did, name: `${did.slice(0, 16)}…`, avatar: "" };
  };

  const isGroup = !!activeRoomId;

  if (!profile) {
    return <div className="mt-24 text-center text-sm text-muted">Sign in to chat.</div>;
  }

  if (!myDid) {
    return (
      <div className="mx-auto mt-24 max-w-md animate-fade-in rounded-2xl border border-amber-400/30 bg-amber-400/10 p-6 text-center">
        <p className="text-sm text-primary">
          Your account is missing a federation identity. Sign out and back in to
          enable chat — your existing recovery key derives it automatically.
        </p>
      </div>
    );
  }

  return (
    <div className="mx-auto grid max-w-6xl gap-4 sm:grid-cols-[300px,1fr]">
      {/* ────────────────────────── Sidebar ────────────────────────── */}
      <aside className="frosted-card flex h-[72vh] flex-col p-3">
        {/* Profile header: avatar opens a popover with did:key + settings */}
        <div className="relative mb-3 flex items-center gap-2 px-1">
          <button
            ref={profileButtonRef}
            type="button"
            onClick={() => setProfileOpen((v) => !v)}
            className={`group flex flex-1 items-center gap-2 rounded-2xl px-2 py-1.5 text-left transition ${
              profileOpen
                ? "bg-accent/10 ring-1 ring-accent/30"
                : "hover:bg-black/[0.04] dark:hover:bg-white/[0.04]"
            }`}
            aria-label="Open profile menu"
          >
            <Avatar
              name={myName}
              avatar={profile?.user?.avatar || ""}
              did={myDid}
              size="h-9 w-9"
            />
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-semibold text-primary">{myName || "you"}</div>
              <div className="truncate text-[10px] text-muted">tap for settings · did:key</div>
            </div>
          </button>
          <ProfilePopover
            open={profileOpen}
            onClose={() => setProfileOpen(false)}
            anchorRef={profileButtonRef}
          />
        </div>

        <div className="flex items-center justify-between px-1 pb-2">
          <h2 className="text-xs font-semibold uppercase tracking-[0.18em] text-muted">Chats</h2>
          <button
            type="button"
            onClick={() => setAddOpen(true)}
            className="rounded-full bg-accent/15 px-3 py-1 text-xs font-semibold text-accent transition hover:bg-accent/25"
          >
            + Add
          </button>
        </div>

        <div className="flex-1 space-y-1 overflow-y-auto pr-1">
          {threads.length === 0 ? (
            <div className="mt-6 rounded-2xl border border-dashed border-black/10 px-3 py-8 text-center text-xs text-muted dark:border-white/10">
              <p className="font-semibold text-primary">No chats yet</p>
              <p className="mt-1">
                Tap <span className="font-semibold text-accent">+ Add</span> to start one
                with a friend's did:key.
              </p>
            </div>
          ) : (
            <>
              {threads.map((t) => {
                const active = activePeerDid === t.peer_did;
                return (
                  <button
                    key={t.peer_did}
                    type="button"
                    onClick={() => setActiveConvo({ type: "dm", id: t.peer_did })}
                    className={`group flex w-full items-center gap-3 rounded-2xl p-2 text-left transition ${
                      active
                        ? "bg-accent/15 ring-1 ring-accent/30"
                        : "hover:bg-black/[0.04] dark:hover:bg-white/[0.04]"
                    }`}
                  >
                    <Avatar name={t.peer_name} avatar={t.peer_avatar} did={t.peer_did} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline gap-2">
                        <div className="truncate text-sm font-semibold text-primary">{t.peer_name}</div>
                        <div className="ml-auto flex-none text-[10px] text-muted">{relativeTime(t.ts)}</div>
                      </div>
                      <div className="truncate text-xs text-muted">{t.last_message}</div>
                    </div>
                  </button>
                );
              })}
            </>
          )}

          {/* Groups section */}
          <div className="flex items-center justify-between px-1 pb-1 pt-3">
            <h2 className="text-xs font-semibold uppercase tracking-[0.18em] text-muted">Groups</h2>
            <button
              type="button"
              onClick={() => setCreateRoomOpen(true)}
              className="rounded-full bg-accent/15 px-3 py-1 text-xs font-semibold text-accent transition hover:bg-accent/25"
            >
              + New
            </button>
          </div>
          {rooms.length === 0 ? (
            <div className="rounded-2xl border border-dashed border-black/10 px-3 py-4 text-center text-[11px] text-muted dark:border-white/10">
              No groups yet.
            </div>
          ) : (
            rooms.map((r) => {
              const active = activeRoomId === r.id;
              return (
                <button
                  key={r.id}
                  type="button"
                  onClick={() => setActiveConvo({ type: "room", id: r.id })}
                  className={`group flex w-full items-center gap-3 rounded-2xl p-2 text-left transition ${
                    active
                      ? "bg-accent/15 ring-1 ring-accent/30"
                      : "hover:bg-black/[0.04] dark:hover:bg-white/[0.04]"
                  }`}
                >
                  <Avatar name={r.name} did={r.id} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-baseline gap-2">
                      <div className="truncate text-sm font-semibold text-primary">{r.name}</div>
                      <div className="ml-auto flex-none text-[10px] text-muted">{relativeTime(r.ts)}</div>
                    </div>
                    <div className="truncate text-xs text-muted">
                      {r.last_message || `${r.member_count} members`}
                    </div>
                  </div>
                </button>
              );
            })
          )}
        </div>

      </aside>

      {/* ────────────────────────── Thread pane ────────────────────────── */}
      <section className="frosted-card flex h-[72vh] flex-col">
        {!convoData ? (
          <div className="grid flex-1 place-items-center px-6">
            <div className="max-w-sm text-center text-sm text-muted">
              <div className="mb-4 grid place-items-center">
                <div className="grid h-16 w-16 place-items-center rounded-full bg-accent/15 text-accent">
                  <PaperPlaneIcon className="h-7 w-7" />
                </div>
              </div>
              <p className="font-semibold text-primary">Pick a conversation</p>
              <p className="mt-1">
                Tap <span className="font-semibold text-accent">+ Add</span> for a 1:1 chat
                or <span className="font-semibold text-accent">+ New</span> for a group.
              </p>
            </div>
          </div>
        ) : (
          <>
            <header className="flex items-center gap-3 border-b border-black/5 px-5 py-4 dark:border-white/10">
              {isGroup ? (
                <>
                  <Avatar name={roomData.room.name} did={roomData.room.id} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-semibold text-primary">{roomData.room.name}</div>
                    <div className="truncate text-[10px] text-muted">
                      {roomData.room.member_count} members · created by {roomData.room.creator_name}
                    </div>
                  </div>
                </>
              ) : (
                <>
                  <Avatar name={threadData.peer.name} avatar={threadData.peer.avatar} did={threadData.peer.did} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-semibold text-primary">{threadData.peer.name}</div>
                    <div className="truncate font-mono text-[10px] text-muted" title={threadData.peer.did}>
                      {DID_TRUNC(threadData.peer.did)}
                    </div>
                  </div>
                </>
              )}
              <span
                className="flex items-center gap-1.5 rounded-full bg-amber-400/15 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider text-amber-700 dark:text-amber-300"
                title="Phase 2: server-attested local mode. Phase 3 will add end-to-end Ed25519 signatures."
              >
                <ShieldCheckIcon className="h-3 w-3" />
                local
              </span>
            </header>

            <div className="flex-1 space-y-4 overflow-y-auto px-4 py-5">
              {convoMessages.length === 0 ? (
                <div className="mt-8 text-center text-xs text-muted">
                  <p>No messages yet.</p>
                  <p className="mt-1 text-primary/70">Say hi — they'll see it within a few seconds.</p>
                </div>
              ) : (
                grouped.map((day, dayIdx) => (
                  <div key={dayIdx} className="space-y-3">
                    <div className="flex items-center gap-3">
                      <div className="h-px flex-1 bg-black/5 dark:bg-white/5" />
                      <span className="text-[10px] font-semibold uppercase tracking-wider text-muted">{day.day}</span>
                      <div className="h-px flex-1 bg-black/5 dark:bg-white/5" />
                    </div>
                    {day.clusters.map((cluster, clusterIdx) => {
                      const mine = cluster.sender_did === myDid;
                      const isLatestCluster = dayIdx === grouped.length - 1 && clusterIdx === day.clusters.length - 1;
                      return (
                        <div key={clusterIdx} className={`flex gap-2 ${mine ? "justify-end" : "justify-start"}`}>
                          {!mine && (() => {
                            const sender = senderInfo(cluster.sender_did);
                            return (
                              <Avatar
                                name={sender.name}
                                avatar={sender.avatar}
                                did={sender.did}
                                size="h-7 w-7"
                                textSize="text-xs"
                              />
                            );
                          })()}
                          <div className={`flex max-w-[70%] flex-col ${mine ? "items-end" : "items-start"}`}>
                            {isGroup && !mine && (
                              <div className="mb-0.5 px-2 text-[11px] font-semibold text-primary/80">
                                {senderInfo(cluster.sender_did).name}
                              </div>
                            )}
                            {cluster.messages.map((m, mIdx) => {
                              const isLast = mIdx === cluster.messages.length - 1;
                              const animate = isLatestCluster && isLast ? "animate-fade-up" : "";
                              const roundedShape = mine
                                ? `rounded-2xl ${isLast ? "rounded-br-md" : ""} ${mIdx === 0 ? "" : "rounded-tr-md"}`
                                : `rounded-2xl ${isLast ? "rounded-bl-md" : ""} ${mIdx === 0 ? "" : "rounded-tl-md"}`;
                              const isEditing = editingId === m.id;
                              const isDeleted = !!m.deleted_at;
                              const canEdit = mine && !isDeleted && (Date.now() - m.ts) < EDIT_WINDOW_MS;
                              const repliedTo = m.reply_to ? messagesById.get(m.reply_to) : null;
                              const reactionEntries = Object.entries(m.reactions || {});
                              const totalReactions = reactionEntries.reduce((a, [, list]) => a + list.length, 0);

                              return (
                                <div key={m.id} className={`group/msg relative ${animate} mt-0.5 flex flex-col ${mine ? "items-end" : "items-start"}`}>
                                  {/* Hover toolbar (chat actions) */}
                                  {!isDeleted && !isEditing && (
                                    <div
                                      className={`pointer-events-none absolute top-1 z-10 flex items-center gap-0.5 rounded-full border border-black/10 bg-white/95 px-1 py-1 opacity-0 shadow-md backdrop-blur-md transition group-hover/msg:pointer-events-auto group-hover/msg:opacity-100 dark:border-white/10 dark:bg-neutral-900/95 ${
                                        mine ? "right-full mr-2" : "left-full ml-2"
                                      }`}
                                    >
                                      <button
                                        type="button"
                                        onClick={() => setPickerForId(pickerForId === m.id ? null : m.id)}
                                        title="React"
                                        className="grid h-7 w-7 place-items-center rounded-full text-base hover:bg-black/5 dark:hover:bg-white/10"
                                      >
                                        😊
                                      </button>
                                      <button
                                        type="button"
                                        onClick={() => handleReplyClick(m)}
                                        title="Reply"
                                        className="grid h-7 w-7 place-items-center rounded-full text-xs font-bold text-muted hover:bg-black/5 hover:text-primary dark:hover:bg-white/10"
                                      >
                                        ↩
                                      </button>
                                      {canEdit && (
                                        <button
                                          type="button"
                                          onClick={() => handleEditStart(m)}
                                          title="Edit"
                                          className="grid h-7 w-7 place-items-center rounded-full text-xs text-muted hover:bg-black/5 hover:text-primary dark:hover:bg-white/10"
                                        >
                                          ✎
                                        </button>
                                      )}
                                      {mine && (
                                        <button
                                          type="button"
                                          onClick={() => handleDelete(m.id)}
                                          title="Delete"
                                          className="grid h-7 w-7 place-items-center rounded-full text-muted hover:bg-red-500/10 hover:text-red-500 dark:hover:bg-red-500/20"
                                        >
                                          <TrashIcon className="h-3.5 w-3.5" />
                                        </button>
                                      )}
                                    </div>
                                  )}

                                  {pickerForId === m.id && (
                                    <ReactionPicker
                                      onPick={(emoji) => handleReact(m.id, emoji)}
                                      onClose={() => setPickerForId(null)}
                                    />
                                  )}

                                  {/* The bubble itself */}
                                  <div
                                    className={`relative px-4 py-2 text-sm leading-snug ${roundedShape} ${
                                      isDeleted
                                        ? "bg-black/[0.03] italic text-muted ring-1 ring-black/5 dark:bg-white/[0.04] dark:ring-white/10"
                                        : mine
                                          ? "bg-amber-400/20 text-amber-900 ring-1 ring-amber-400/40 dark:text-amber-100"
                                          : "bg-black/[0.04] text-primary ring-1 ring-black/5 dark:bg-white/[0.06] dark:ring-white/10"
                                    }`}
                                    title={new Date(m.ts).toLocaleString()}
                                  >
                                    {/* Reply context (quoted preview) */}
                                    {repliedTo && (
                                      <div className={`mb-2 rounded-md border-l-2 px-2 py-1 text-xs ${
                                        mine
                                          ? "border-amber-500/60 bg-amber-500/10 text-amber-900/80 dark:text-amber-100/80"
                                          : "border-accent/50 bg-black/[0.03] text-muted dark:bg-white/[0.04]"
                                      }`}>
                                        <div className="text-[10px] font-semibold uppercase tracking-wider opacity-70">
                                          {repliedTo.sender_did === myDid ? "you" : senderInfo(repliedTo.sender_did).name}
                                        </div>
                                        <div className="truncate">
                                          {repliedTo.deleted_at ? <em>deleted message</em> : (repliedTo.content || "")}
                                        </div>
                                      </div>
                                    )}

                                    {isEditing ? (
                                      <div className="flex flex-col gap-2">
                                        <textarea
                                          ref={editRef}
                                          value={editDraft}
                                          onChange={(e) => setEditDraft(e.target.value)}
                                          onKeyDown={(e) => {
                                            if (e.key === "Escape") handleEditCancel();
                                            if (e.key === "Enter" && !e.shiftKey) {
                                              e.preventDefault();
                                              handleEditSave();
                                            }
                                          }}
                                          rows={2}
                                          className="w-64 resize-none rounded-md bg-white/60 px-2 py-1 text-sm text-primary outline-none ring-1 ring-amber-400 dark:bg-black/40"
                                        />
                                        <div className="flex justify-end gap-1">
                                          <button
                                            type="button"
                                            onClick={handleEditCancel}
                                            className="rounded-full px-3 py-1 text-xs text-muted hover:bg-black/5 dark:hover:bg-white/10"
                                          >
                                            cancel
                                          </button>
                                          <button
                                            type="button"
                                            onClick={handleEditSave}
                                            className="rounded-full bg-accent px-3 py-1 text-xs font-semibold text-accent-text hover:bg-amber-300"
                                          >
                                            save
                                          </button>
                                        </div>
                                      </div>
                                    ) : (
                                      <>
                                        {/* Attachments grid: 1 = full, 2 = side-by-side, 3-4 = 2x2 */}
                                        {!isDeleted && m.attachments && m.attachments.length > 0 && (
                                          <div
                                            className={`mb-2 grid gap-1 overflow-hidden rounded-lg ${
                                              m.attachments.length === 1
                                                ? "grid-cols-1"
                                                : "grid-cols-2"
                                            }`}
                                            style={{ maxWidth: m.attachments.length === 1 ? 320 : 280 }}
                                          >
                                            {m.attachments.map((a, ai) => (
                                              <AttachmentTile key={ai} attachment={a} />
                                            ))}
                                          </div>
                                        )}
                                        {(isDeleted || m.content) && (
                                          <div className="whitespace-pre-wrap break-words">
                                            {isDeleted ? "message deleted" : m.content}
                                          </div>
                                        )}
                                      </>
                                    )}
                                  </div>

                                  {/* Reactions row */}
                                  {totalReactions > 0 && !isDeleted && (
                                    <div className={`mt-1 flex flex-wrap gap-1 ${mine ? "justify-end" : "justify-start"}`}>
                                      {reactionEntries.map(([emoji, list]) => {
                                        const mineReacted = list.includes(myDid);
                                        return (
                                          <button
                                            key={emoji}
                                            type="button"
                                            onClick={() => handleReact(m.id, emoji)}
                                            className={`flex items-center gap-1 rounded-full px-2 py-0.5 text-xs transition ${
                                              mineReacted
                                                ? "bg-amber-400/30 ring-1 ring-amber-400/60"
                                                : "bg-black/5 ring-1 ring-black/5 hover:bg-black/10 dark:bg-white/[0.06] dark:ring-white/10 dark:hover:bg-white/10"
                                            }`}
                                            title={`${list.length} · click to ${mineReacted ? "remove" : "add"}`}
                                          >
                                            <span>{emoji}</span>
                                            <span className="text-[10px] font-semibold opacity-80">{list.length}</span>
                                          </button>
                                        );
                                      })}
                                    </div>
                                  )}
                                </div>
                              );
                            })}
                            <div className="mt-1 flex items-center gap-1.5 px-2 text-[10px] text-muted">
                              <span>
                                {new Date(
                                  cluster.messages[cluster.messages.length - 1].ts
                                ).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
                              </span>
                              {cluster.messages.some((m) => m.edited_at) && (
                                <span className="italic opacity-70">· edited</span>
                              )}
                              {/* Read receipts: ✓ sent, ✓✓ read by peer (only on mine) */}
                              {mine && !cluster.messages[cluster.messages.length - 1].deleted_at && (
                                cluster.messages[cluster.messages.length - 1].read_at ? (
                                  <span className="text-sky-500 dark:text-sky-400" title="Read">✓✓</span>
                                ) : (
                                  <span className="opacity-70" title="Sent">✓</span>
                                )
                              )}
                            </div>
                          </div>
                          {mine && (
                            <Avatar name={myName} did={myDid} size="h-7 w-7" textSize="text-xs" />
                          )}
                        </div>
                      );
                    })}
                  </div>
                ))
              )}
              <div ref={threadEndRef} />
            </div>

            {error && (
              <p className="mx-4 mb-2 animate-fade-in text-xs text-red-500 dark:text-red-400">{error}</p>
            )}

            {/* Pending attachments preview row */}
            {pendingFiles.length > 0 && (
              <div className="mx-3 mb-2 flex flex-wrap gap-2 rounded-xl border border-black/5 bg-black/[0.02] p-2 dark:border-white/10 dark:bg-white/[0.03]">
                {filePreviewUrls.map((p, idx) => {
                  const isVideo = p.file.type.startsWith("video/");
                  return (
                    <div
                      key={idx}
                      className="group/preview relative h-16 w-16 overflow-hidden rounded-lg ring-1 ring-black/10 dark:ring-white/10"
                    >
                      {isVideo ? (
                        <div className="grid h-full w-full place-items-center bg-black text-white">
                          <PlayIcon className="h-6 w-6" />
                        </div>
                      ) : (
                        <img src={p.url} alt="" className="h-full w-full object-cover" />
                      )}
                      <button
                        type="button"
                        onClick={() => handleRemovePendingFile(idx)}
                        className="absolute right-0.5 top-0.5 grid h-5 w-5 place-items-center rounded-full bg-black/70 text-white opacity-0 transition group-hover/preview:opacity-100"
                        aria-label="Remove"
                      >
                        <CloseIcon className="h-3 w-3" />
                      </button>
                    </div>
                  );
                })}
                {sending && uploadProgress > 0 && (
                  <div className="ml-auto self-center text-xs text-muted">
                    uploading… {uploadProgress}%
                  </div>
                )}
              </div>
            )}

            {/* Reply context above compose */}
            {replyingTo && (
              <div className="mx-3 mb-2 flex items-start gap-2 rounded-xl border-l-2 border-accent bg-accent/10 px-3 py-2">
                <div className="flex-1 min-w-0">
                  <div className="text-[10px] font-semibold uppercase tracking-wider text-accent">
                    replying to {replyingTo.sender_did === myDid ? "yourself" : senderInfo(replyingTo.sender_did).name}
                  </div>
                  <div className="truncate text-xs text-muted">
                    {replyingTo.deleted_at ? <em>deleted message</em> : (replyingTo.content || "")}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => setReplyingTo(null)}
                  className="grid h-6 w-6 place-items-center rounded-full text-muted hover:bg-black/5 dark:hover:bg-white/10"
                  aria-label="Cancel reply"
                >
                  <CloseIcon className="h-3 w-3" />
                </button>
              </div>
            )}

            {recState === "recording" ? (
              <div className="flex items-center gap-3 border-t border-black/5 px-3 py-3 dark:border-white/10">
                <button
                  type="button"
                  onClick={handleCancelRecording}
                  aria-label="Cancel recording"
                  title="Cancel"
                  className="grid h-10 w-10 flex-none place-items-center rounded-full text-muted transition hover:bg-black/5 hover:text-red-500 dark:hover:bg-white/10"
                >
                  <CloseIcon className="h-4 w-4" />
                </button>
                <div className="flex flex-1 items-center gap-2 rounded-full bg-red-500/10 px-4 py-2 ring-1 ring-red-500/30">
                  <span className="h-2.5 w-2.5 animate-pulse rounded-full bg-red-500" />
                  <span className="text-xs font-semibold text-red-700 dark:text-red-300">Recording</span>
                  <span className="ml-auto font-mono text-xs text-red-700 dark:text-red-300">
                    {formatDuration(recDuration)}
                  </span>
                </div>
                <button
                  type="button"
                  onClick={handleStopRecording}
                  aria-label="Stop recording"
                  title="Stop"
                  className="grid h-10 w-10 flex-none place-items-center rounded-full bg-red-500 text-white transition hover:bg-red-600 active:scale-95"
                >
                  <StopIcon className="h-4 w-4" />
                </button>
              </div>
            ) : recState === "preview" ? (
              <div className="flex items-center gap-2 border-t border-black/5 px-3 py-3 dark:border-white/10">
                <button
                  type="button"
                  onClick={handleCancelRecording}
                  aria-label="Discard"
                  title="Discard"
                  className="grid h-10 w-10 flex-none place-items-center rounded-full text-muted transition hover:bg-red-500/10 hover:text-red-500"
                >
                  <TrashIcon className="h-4 w-4" />
                </button>
                <div className="flex flex-1 items-center gap-2 rounded-full bg-black/[0.04] px-3 py-1.5 ring-1 ring-black/5 dark:bg-white/[0.06] dark:ring-white/10">
                  {voicePreviewUrlRef.current && (
                    <audio
                      src={voicePreviewUrlRef.current}
                      controls
                      className="h-8 flex-1"
                    />
                  )}
                  <span className="font-mono text-[10px] text-muted">{formatDuration(recDuration)}</span>
                </div>
                <button
                  type="button"
                  onClick={handleSendVoice}
                  disabled={sending}
                  aria-label="Send voice"
                  title="Send voice"
                  className="grid h-10 w-10 flex-none place-items-center rounded-full bg-accent text-accent-text transition hover:bg-amber-300 active:scale-95 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  <PaperPlaneIcon className="h-4 w-4" />
                </button>
              </div>
            ) : (
              <form
                onSubmit={handleSend}
                className="relative flex items-end gap-2 border-t border-black/5 px-3 py-3 dark:border-white/10"
              >
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*,video/mp4,video/quicktime,video/webm"
                  multiple
                  onChange={handleFilesChosen}
                  className="hidden"
                />

                {/* Single "+" attach menu — photo/video OR voice */}
                <div className="relative">
                  <button
                    type="button"
                    onClick={() => setAttachMenuOpen((v) => !v)}
                    disabled={sending}
                    aria-label="Attach"
                    title="Attach photo, video, or voice"
                    className={`grid h-10 w-10 flex-none place-items-center rounded-full transition active:scale-95 disabled:cursor-not-allowed disabled:opacity-40 ${
                      attachMenuOpen
                        ? "rotate-45 bg-accent/20 text-accent"
                        : "text-muted hover:bg-black/5 hover:text-primary dark:hover:bg-white/10"
                    }`}
                  >
                    <PlusIcon className="h-5 w-5" />
                  </button>
                  {attachMenuOpen && (
                    <>
                      {/* Outside-click backdrop */}
                      <div
                        className="fixed inset-0 z-10"
                        onClick={() => setAttachMenuOpen(false)}
                      />
                      <div className="absolute bottom-12 left-0 z-20 w-44 animate-pop-in overflow-hidden rounded-2xl border border-black/10 bg-white/95 shadow-xl backdrop-blur-xl dark:border-white/15 dark:bg-neutral-900/95">
                        <button
                          type="button"
                          onClick={() => {
                            setAttachMenuOpen(false);
                            handlePickFiles();
                          }}
                          disabled={pendingFiles.length >= MAX_ATTACHMENTS}
                          className="flex w-full items-center gap-3 px-3 py-2.5 text-sm text-primary transition hover:bg-black/5 disabled:cursor-not-allowed disabled:opacity-40 dark:hover:bg-white/10"
                        >
                          <ImageIcon className="h-4 w-4 text-muted" />
                          <span>Photo or video</span>
                        </button>
                        <button
                          type="button"
                          onClick={() => {
                            setAttachMenuOpen(false);
                            handleStartRecording();
                          }}
                          className="flex w-full items-center gap-3 px-3 py-2.5 text-sm text-primary transition hover:bg-black/5 dark:hover:bg-white/10"
                        >
                          <MicIcon className="h-4 w-4 text-muted" />
                          <span>Voice message</span>
                        </button>
                      </div>
                    </>
                  )}
                </div>

                <textarea
                  ref={composeRef}
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder="Type a message…   ↵ to send · Shift+↵ for newline"
                  disabled={sending}
                  rows={1}
                  maxLength={2000}
                  className="frosted-input flex-1 resize-none text-sm leading-snug"
                  style={{ minHeight: "40px" }}
                />
                {draft.length > 1800 && (
                  <span className="text-[10px] text-muted">{2000 - draft.length}</span>
                )}
                <button
                  type="submit"
                  disabled={sending || (!draft.trim() && pendingFiles.length === 0)}
                  aria-label="Send"
                  title="Send (Enter)"
                  className="grid h-10 w-10 flex-none place-items-center rounded-full bg-accent text-accent-text transition hover:bg-amber-300 active:scale-95 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  <PaperPlaneIcon className="h-4 w-4" />
                </button>
              </form>
            )}
          </>
        )}
      </section>

      {addOpen && (
        <AddFriendModal
          token={token}
          onClose={() => setAddOpen(false)}
          onAdded={handleAdded}
        />
      )}

      {createRoomOpen && (
        <CreateRoomModal
          token={token}
          onClose={() => setCreateRoomOpen(false)}
          onCreated={handleRoomCreated}
        />
      )}
    </div>
  );
};

export default Chat;
