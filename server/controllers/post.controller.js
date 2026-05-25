const { randomUUID } = require("crypto");
const { readDb, writeDb } = require("../utils/db");
const { createNotification } = require("../utils/notifications");
const fs = require("fs/promises");
const path = require("path");
const env = require("../utils/env");
const { processFile } = require("../utils/media");

const canSeePost = (post, viewerId) => {
  if (!viewerId) return false;
  if (post.userId === viewerId) return true;
  return false;
};

const buildFollowSetFor = (db, viewerId) => {
  const me = db.users.find((u) => u.id === viewerId);
  if (!me) return new Set();
  const followingIds = Array.isArray(me.following) ? me.following : [];
  const set = new Set(followingIds);
  set.add(viewerId);
  return set;
};

const isFollowerOrSelf = (db, postOwnerId, viewerId) => {
  if (!viewerId) return false;
  if (postOwnerId === viewerId) return true;
  const owner = db.users.find((u) => u.id === postOwnerId);
  if (!owner) return false;
  return Array.isArray(owner.followers) && owner.followers.includes(viewerId);
};

const userCoverFor = (post) => post.images?.[0]?.url || "";

const userInfoCache = (db, userId) => {
  const u = db.users.find((x) => x.id === userId);
  return {
    fromUserId: userId,
    fromUserName: u?.name || "",
    fromUserAvatar: u?.avatar || "",
  };
};

const MAX_IMAGES = 12;
const HASHTAG_RE = /#([\p{L}\p{N}_]+)/gu;

const extractHashtags = (text) => {
  const tags = new Set();
  let match;
  while ((match = HASHTAG_RE.exec(text || "")) !== null) {
    tags.add(match[1].toLowerCase());
  }
  return [...tags];
};

const createPost = async (req, res) => {
  try {
    const captionRaw = typeof req.body.caption === "string" ? req.body.caption.trim() : "";
    const caption = captionRaw.slice(0, 2200);
    const files = req.files || [];

    if (files.length === 0) {
      return res.status(400).json({ message: "At least one file is required" });
    }
    if (files.length > MAX_IMAGES) {
      return res.status(400).json({ message: `Maximum ${MAX_IMAGES} files per post` });
    }

    const uploadsDir = env.UPLOADS_DIR;
    await fs.mkdir(uploadsDir, { recursive: true });

    const images = await Promise.all(files.map((file) => processFile(file, uploadsDir)));

    const db = await readDb();
    const author = db.users.find((u) => u.id === req.user.id);
    const newPost = {
      id: randomUUID(),
      userId: req.user.id,
      userName: req.user.name,
      userAvatar: author?.avatar || "",
      caption,
      hashtags: extractHashtags(caption),
      images,
      reactions: {},
      reposts: [],
      lastRepostAt: null,
      comments: [],
      createdAt: new Date().toISOString(),
    };

    db.posts.push(newPost);
    await writeDb(db);

    res.status(201).json({ message: "Post created successfully", post: newPost });
  } catch (error) {
    res.status(500).json({ message: "Unable to create post" });
  }
};

const sortValue = (post) =>
  new Date(post.lastRepostAt || post.createdAt).getTime();

const normalizePost = (post, usersById) => {
  const images = Array.isArray(post.images)
    ? post.images.map((img) => ({ type: img.type || "photo", ...img }))
    : post.imageUrl
    ? [{ url: post.imageUrl, type: "photo" }]
    : [];
  const fallbackAuthor = usersById && usersById.get
    ? usersById.get(post.userId)
    : null;
  return {
    id: post.id,
    userId: post.userId,
    userName: fallbackAuthor?.name || post.userName,
    userAvatar: fallbackAuthor?.avatar || post.userAvatar || "",
    caption: post.caption ?? post.text ?? "",
    hashtags: post.hashtags || extractHashtags(post.caption ?? post.text ?? ""),
    images,
    reactions: post.reactions || {},
    reposts: post.reposts || [],
    lastRepostAt: post.lastRepostAt || null,
    comments: (post.comments || []).map((c) => {
      const cu = usersById && usersById.get ? usersById.get(c.userId) : null;
      return {
        ...c,
        userName: cu?.name || c.userName,
        userAvatar: cu?.avatar || c.userAvatar || "",
      };
    }),
    createdAt: post.createdAt,
  };
};

const buildUsersById = (db) => new Map((db.users || []).map((u) => [u.id, u]));

const getPosts = async (req, res) => {
  const db = await readDb();
  if (!req.user) {
    return res.status(200).json({ posts: [] });
  }
  const usersById = buildUsersById(db);
  const visibleSet = buildFollowSetFor(db, req.user.id);
  const posts = [...db.posts]
    .map((p) => normalizePost(p, usersById))
    .filter((p) => p.images.length > 0 && visibleSet.has(p.userId))
    .sort((a, b) => sortValue(b) - sortValue(a));
  res.status(200).json({ posts });
};

const getPost = async (req, res) => {
  const db = await readDb();
  const raw = db.posts.find((p) => p.id === req.params.id);
  if (!raw) return res.status(404).json({ message: "Post not found" });
  if (!isFollowerOrSelf(db, raw.userId, req.user?.id)) {
    return res.status(403).json({ message: "Private post" });
  }
  res.status(200).json({ post: normalizePost(raw, buildUsersById(db)) });
};

const getUserPosts = async (req, res) => {
  const db = await readDb();
  const { id } = req.params;
  if (!isFollowerOrSelf(db, id, req.user?.id)) {
    return res.status(200).json({ posts: [], private: true });
  }
  const usersById = buildUsersById(db);
  const posts = [...db.posts]
    .map((p) => normalizePost(p, usersById))
    .filter((p) => p.images.length > 0 && p.userId === id)
    .sort((a, b) => sortValue(b) - sortValue(a));
  res.status(200).json({ posts, private: false });
};

const reactToPost = async (req, res) => {
  const { emoji: rawEmoji } = req.body;
  if (typeof rawEmoji !== "string" || !rawEmoji.trim()) {
    return res.status(400).json({ message: "Emoji is required" });
  }
  // Cap to ~16 UTF-16 code units; even complex ZWJ sequences fit comfortably.
  const emoji = rawEmoji.trim().slice(0, 16);

  const db = await readDb();
  const post = db.posts.find((p) => p.id === req.params.id);
  if (!post) return res.status(404).json({ message: "Post not found" });
  if (!isFollowerOrSelf(db, post.userId, req.user.id)) {
    return res.status(403).json({ message: "Private post" });
  }

  if (!post.reactions) post.reactions = {};
  const list = post.reactions[emoji] || [];
  const idx = list.indexOf(req.user.id);
  let added = false;
  if (idx >= 0) {
    list.splice(idx, 1);
  } else {
    list.push(req.user.id);
    added = true;
  }

  if (list.length === 0) {
    delete post.reactions[emoji];
  } else {
    post.reactions[emoji] = list;
  }

  if (added) {
    createNotification(db, {
      ...userInfoCache(db, req.user.id),
      userId: post.userId,
      type: "reaction",
      postId: post.id,
      postCover: userCoverFor(post),
      emoji,
    });
  }

  await writeDb(db);
  res.status(200).json({ post: normalizePost(post, buildUsersById(db)) });
};

const repostPost = async (req, res) => {
  const db = await readDb();
  const post = db.posts.find((p) => p.id === req.params.id);
  if (!post) return res.status(404).json({ message: "Post not found" });
  if (!isFollowerOrSelf(db, post.userId, req.user.id)) {
    return res.status(403).json({ message: "Private post" });
  }

  if (!Array.isArray(post.reposts)) post.reposts = [];
  const idx = post.reposts.findIndex((r) => r.userId === req.user.id);
  let added = false;

  if (idx >= 0) {
    post.reposts.splice(idx, 1);
    post.lastRepostAt = post.reposts.length
      ? post.reposts[post.reposts.length - 1].repostedAt
      : null;
  } else {
    const entry = {
      userId: req.user.id,
      userName: req.user.name,
      repostedAt: new Date().toISOString(),
    };
    post.reposts.push(entry);
    post.lastRepostAt = entry.repostedAt;
    added = true;
  }

  if (added) {
    createNotification(db, {
      ...userInfoCache(db, req.user.id),
      userId: post.userId,
      type: "repost",
      postId: post.id,
      postCover: userCoverFor(post),
    });
  }

  await writeDb(db);
  res.status(200).json({ post: normalizePost(post, buildUsersById(db)) });
};

const addComment = async (req, res) => {
  const text = typeof req.body.text === "string" ? req.body.text.trim() : "";
  if (!text) return res.status(400).json({ message: "Comment text is required" });
  if (text.length > 500) return res.status(400).json({ message: "Comment too long" });

  const db = await readDb();
  const post = db.posts.find((p) => p.id === req.params.id);
  if (!post) return res.status(404).json({ message: "Post not found" });
  if (!isFollowerOrSelf(db, post.userId, req.user.id)) {
    return res.status(403).json({ message: "Private post" });
  }

  if (!Array.isArray(post.comments)) post.comments = [];

  // Optional threading: parentId must reference an existing top-level comment
  // on this post. We flatten the tree (no nested-of-nested) — replies-to-replies
  // collapse onto the original parent for predictable rendering.
  let parentId = null;
  let notifyUserId = post.userId; // by default the post owner gets the comment notification
  if (typeof req.body.parentId === "string" && req.body.parentId) {
    const parent = post.comments.find((c) => c.id === req.body.parentId);
    if (!parent) return res.status(400).json({ message: "Parent comment not found" });
    parentId = parent.parentId || parent.id; // collapse: always reply to the root
    notifyUserId = parent.userId;
  }

  const commenter = db.users.find((u) => u.id === req.user.id);
  const comment = {
    id: randomUUID(),
    userId: req.user.id,
    userName: req.user.name,
    userAvatar: commenter?.avatar || "",
    parentId,
    text,
    createdAt: new Date().toISOString(),
  };
  post.comments.push(comment);

  // Don't self-notify if user replies to their own comment / post
  if (notifyUserId && notifyUserId !== req.user.id) {
    createNotification(db, {
      ...userInfoCache(db, req.user.id),
      userId: notifyUserId,
      type: "comment",
      postId: post.id,
      postCover: userCoverFor(post),
      commentText: text.slice(0, 140),
    });
  }

  await writeDb(db);
  res.status(201).json({ comment, post: normalizePost(post, buildUsersById(db)) });
};

const deleteComment = async (req, res) => {
  const db = await readDb();
  const post = db.posts.find((p) => p.id === req.params.id);
  if (!post) return res.status(404).json({ message: "Post not found" });

  const idx = (post.comments || []).findIndex((c) => c.id === req.params.commentId);
  if (idx < 0) return res.status(404).json({ message: "Comment not found" });

  const comment = post.comments[idx];
  if (comment.userId !== req.user.id && post.userId !== req.user.id) {
    return res.status(403).json({ message: "Not allowed" });
  }
  post.comments.splice(idx, 1);

  await writeDb(db);
  res.status(200).json({ post });
};

const deletePost = async (req, res) => {
  const db = await readDb();
  const idx = db.posts.findIndex((p) => p.id === req.params.id);
  if (idx < 0) return res.status(404).json({ message: "Post not found" });

  const post = db.posts[idx];
  if (post.userId !== req.user.id) {
    return res.status(403).json({ message: "Not allowed" });
  }

  for (const img of post.images || []) {
    if (img.url?.startsWith("/uploads/")) {
      const relative = img.url.slice("/uploads/".length);
      const filePath = path.resolve(path.join(env.UPLOADS_DIR, relative));
      if (filePath.startsWith(env.UPLOADS_DIR + path.sep)) {
        fs.unlink(filePath).catch(() => {});
      }
    }
  }

  db.posts.splice(idx, 1);
  await writeDb(db);
  res.status(200).json({ message: "Deleted" });
};

const reactToComment = async (req, res) => {
  const { emoji: rawEmoji } = req.body;
  if (typeof rawEmoji !== "string" || !rawEmoji.trim()) {
    return res.status(400).json({ message: "Emoji is required" });
  }
  const emoji = rawEmoji.trim().slice(0, 16);

  const db = await readDb();
  const post = db.posts.find((p) => p.id === req.params.id);
  if (!post) return res.status(404).json({ message: "Post not found" });
  if (!isFollowerOrSelf(db, post.userId, req.user.id)) {
    return res.status(403).json({ message: "Private post" });
  }

  const comment = (post.comments || []).find((c) => c.id === req.params.commentId);
  if (!comment) return res.status(404).json({ message: "Comment not found" });

  if (!comment.reactions || typeof comment.reactions !== "object") comment.reactions = {};
  const list = comment.reactions[emoji] || [];
  const has = list.includes(req.user.id);
  comment.reactions[emoji] = has
    ? list.filter((id) => id !== req.user.id)
    : [...list, req.user.id];
  if (comment.reactions[emoji].length === 0) delete comment.reactions[emoji];

  // Notify the comment author once per actor (skip self)
  if (!has && comment.userId !== req.user.id) {
    createNotification(db, {
      ...userInfoCache(db, req.user.id),
      userId: comment.userId,
      type: "reaction",
      postId: post.id,
      postCover: userCoverFor(post),
      commentId: comment.id,
      emoji,
    });
  }

  await writeDb(db);
  res.status(200).json({ post: normalizePost(post, buildUsersById(db)) });
};

module.exports = {
  createPost,
  getPosts,
  getPost,
  getUserPosts,
  reactToPost,
  reactToComment,
  repostPost,
  addComment,
  deleteComment,
  deletePost,
};
