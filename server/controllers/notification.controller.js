const { readDb, writeDb } = require("../utils/db");

const MAX_PAGE = 50;
const DEFAULT_PAGE = 20;

const list = async (req, res) => {
  try {
    const db = await readDb();
    const limit = Math.max(1, Math.min(MAX_PAGE, parseInt(req.query.limit, 10) || DEFAULT_PAGE));
    const before = req.query.before ? new Date(req.query.before).getTime() : null;
    const mine = (db.notifications || [])
      .filter((n) => n.userId === req.user.id)
      .filter((n) => !before || new Date(n.createdAt).getTime() < before)
      .sort((a, b) => new Date(b.createdAt) - new Date(a.createdAt));
    const notifications = mine.slice(0, limit);
    res.status(200).json({
      notifications,
      hasMore: mine.length > limit,
      nextBefore:
        notifications.length === limit ? notifications[notifications.length - 1].createdAt : null,
    });
  } catch (error) {
    res.status(500).json({ message: "Unable to load notifications" });
  }
};

const markAllRead = async (req, res) => {
  try {
    const db = await readDb();
    let changed = false;
    for (const n of db.notifications || []) {
      if (n.userId === req.user.id && !n.read) {
        n.read = true;
        changed = true;
      }
    }
    if (changed) await writeDb(db);
    res.status(200).json({ message: "ok" });
  } catch (error) {
    res.status(500).json({ message: "Unable to update" });
  }
};

const remove = async (req, res) => {
  try {
    const db = await readDb();
    const before = (db.notifications || []).length;
    db.notifications = (db.notifications || []).filter(
      (n) => !(n.id === req.params.id && n.userId === req.user.id)
    );
    if (db.notifications.length < before) await writeDb(db);
    res.status(200).json({ message: "ok" });
  } catch (error) {
    res.status(500).json({ message: "Unable to delete" });
  }
};

module.exports = { list, markAllRead, remove };
