const jwt = require("jsonwebtoken");
const { SECRET } = require("../utils/secrets");

module.exports = (req, res, next) => {
  const authHeader = req.headers.authorization || "";
  const token = authHeader.startsWith("Bearer ")
    ? authHeader.slice(7).trim()
    : authHeader.trim();

  if (token) {
    try {
      req.user = jwt.verify(token, SECRET);
    } catch {
      /* ignore invalid token, treat as anonymous */
    }
  }
  next();
};
