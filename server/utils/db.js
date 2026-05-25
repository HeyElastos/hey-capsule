const fs = require("fs/promises");
const path = require("path");
const { Mutex } = require("async-mutex");

const dbPath = path.join(__dirname, "../data/db.json");
const tmpPath = `${dbPath}.tmp`;
const writeLock = new Mutex();

const readDb = async () => {
  try {
    const file = await fs.readFile(dbPath, "utf8");
    const data = JSON.parse(file);
    if (!Array.isArray(data.users)) data.users = [];
    if (!Array.isArray(data.posts)) data.posts = [];
    if (!Array.isArray(data.notifications)) data.notifications = [];
    return data;
  } catch (error) {
    const initial = { users: [], posts: [], notifications: [] };
    await writeDb(initial);
    return initial;
  }
};

// Atomic + serialized: writes go to db.json.tmp then rename. Mutex prevents
// interleaved read-modify-write races between concurrent requests.
const writeDb = async (data) =>
  writeLock.runExclusive(async () => {
    await fs.mkdir(path.dirname(dbPath), { recursive: true });
    await fs.writeFile(tmpPath, JSON.stringify(data, null, 2), "utf8");
    await fs.rename(tmpPath, dbPath);
  });

module.exports = { readDb, writeDb };
