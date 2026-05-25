// Shared media-processing pipeline. Used by post.controller and
// chat.controller so both flows produce identical, browser-safe assets.
//
// Trust no client metadata. Inspect magic bytes (file-type), then transcode
// images → AVIF and videos → H.264 + AAC MP4 with faststart.

const { randomUUID } = require("crypto");
const path = require("path");
const sharp = require("sharp");
const fileType = require("file-type");
const { ensureBrowserSafeVideo } = require("./video");

const VIDEO_EXT_BY_MIME = {
  "video/mp4": ".mp4",
  "video/quicktime": ".mov",
  "video/webm": ".webm",
};

const IMAGE_MIMES_OK = new Set([
  "image/jpeg",
  "image/png",
  "image/webp",
  "image/avif",
  "image/heic",
  "image/heif",
  "image/gif",
]);

// Multipart upload constraints — single source of truth so routes stay in sync.
const ALLOWED_MIMES = new Set([
  ...IMAGE_MIMES_OK,
  "video/mp4",
  "video/quicktime",
  "video/webm",
]);

const processImage = async (file, uploadsDir) => {
  const fileName = `${randomUUID()}.avif`;
  const outputPath = path.join(uploadsDir, fileName);
  const { width, height } = await sharp(file.buffer)
    .rotate()
    .resize({ width: 1600, withoutEnlargement: true })
    .avif({ quality: 65 })
    .toFile(outputPath);
  return { url: `/uploads/${fileName}`, type: "photo", width, height };
};

const processVideo = async (file, uploadsDir, detectedMime) => {
  const ext = VIDEO_EXT_BY_MIME[detectedMime];
  if (!ext) throw new Error("Unsupported video type");
  const { url } = await ensureBrowserSafeVideo(file.buffer, uploadsDir, ext);
  return { url, type: "video", mime: "video/mp4" };
};

const processFile = async (file, uploadsDir) => {
  const detected = await fileType.fromBuffer(file.buffer);
  if (!detected) throw new Error("Could not detect file type");
  const realMime = detected.mime;
  if (VIDEO_EXT_BY_MIME[realMime]) {
    return processVideo(file, uploadsDir, realMime);
  }
  if (IMAGE_MIMES_OK.has(realMime)) {
    return processImage(file, uploadsDir);
  }
  throw new Error(`Disallowed file type: ${realMime}`);
};

module.exports = {
  processFile,
  ALLOWED_MIMES,
  VIDEO_EXT_BY_MIME,
  IMAGE_MIMES_OK,
};
