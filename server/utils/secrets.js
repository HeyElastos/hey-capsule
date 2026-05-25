const crypto = require("crypto");

const isProd = process.env.NODE_ENV === "production";

const resolve = (name) => {
  const v = process.env[name];
  if (v && v.length >= 16) return v;
  if (isProd) {
    // Fail loud — never run prod with a default/weak secret.
    throw new Error(
      `Refusing to start: env ${name} is required (>= 16 chars) in production.`
    );
  }
  // Dev only: generate an ephemeral random secret per process.
  // Tokens issued in one run won't validate in the next — that's the point.
  const generated = crypto.randomBytes(48).toString("hex");
  console.warn(
    `[secrets] ${name} not set; using an ephemeral random value for this process (dev only).`
  );
  return generated;
};

module.exports = {
  SECRET: resolve("SECRET"),
  REFRESH_SECRET: resolve("REFRESH_SECRET"),
};
