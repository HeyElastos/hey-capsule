const { randomUUID } = require("crypto");

const createNotification = (db, payload) => {
  if (!Array.isArray(db.notifications)) db.notifications = [];

  const { userId, fromUserId, type } = payload;
  if (!userId || userId === fromUserId) return null;

  // Dedupe reactions/reposts: collapse repeat events from same actor on same post
  if (type === "reaction" || type === "repost") {
    const existing = db.notifications.find(
      (n) =>
        n.type === type &&
        n.userId === userId &&
        n.fromUserId === fromUserId &&
        n.postId === payload.postId
    );
    if (existing) {
      Object.assign(existing, payload, {
        read: false,
        createdAt: new Date().toISOString(),
      });
      return existing;
    }
  }

  // Dedupe follow_request: collapse repeats
  if (type === "follow_request") {
    const existing = db.notifications.find(
      (n) =>
        n.type === "follow_request" &&
        n.userId === userId &&
        n.fromUserId === fromUserId
    );
    if (existing) {
      existing.read = false;
      existing.createdAt = new Date().toISOString();
      return existing;
    }
  }

  const notification = {
    id: randomUUID(),
    read: false,
    createdAt: new Date().toISOString(),
    ...payload,
  };
  db.notifications.push(notification);

  // Hard-cap each recipient's notifications so the db can't grow unbounded
  // under sustained abuse (see #11 in security audit).
  const MAX_PER_USER = 200;
  const recipientNotifs = db.notifications
    .map((n, idx) => ({ n, idx }))
    .filter(({ n }) => n.userId === userId)
    .sort((a, b) => new Date(b.n.createdAt) - new Date(a.n.createdAt));
  if (recipientNotifs.length > MAX_PER_USER) {
    const keepIds = new Set(recipientNotifs.slice(0, MAX_PER_USER).map(({ n }) => n.id));
    db.notifications = db.notifications.filter(
      (n) => n.userId !== userId || keepIds.has(n.id)
    );
  }

  return notification;
};

const removeFollowRequestNotification = (db, { userId, fromUserId }) => {
  if (!Array.isArray(db.notifications)) return;
  db.notifications = db.notifications.filter(
    (n) =>
      !(
        n.type === "follow_request" &&
        n.userId === userId &&
        n.fromUserId === fromUserId
      )
  );
};

module.exports = { createNotification, removeFollowRequestNotification };
