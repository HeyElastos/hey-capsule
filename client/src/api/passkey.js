import axios from "axios";
import {
  startRegistration,
  startAuthentication,
  browserSupportsWebAuthn,
} from "@simplewebauthn/browser";

const API = axios.create({ baseURL: "/api" });
const authHeaders = (token) => ({ Authorization: `Bearer ${token}` });

export const passkeySupported = () =>
  typeof window !== "undefined" && browserSupportsWebAuthn();

// Sign up a brand-new user with a passkey. Returns the same shape as signup:
//   { user, accessToken, refreshToken }
export const passkeySignup = async (name) => {
  const start = await API.post("/passkey/register/options", { name });
  const { challengeId, options } = start.data;
  const attResp = await startRegistration({ optionsJSON: options });
  const verify = await API.post("/passkey/register/verify", {
    challengeId,
    response: attResp,
  });
  return verify.data;
};

// Attach a passkey to an already-signed-in account.
export const passkeyAttach = async (token) => {
  const start = await API.post(
    "/passkey/register/options",
    {},
    { headers: authHeaders(token) }
  );
  const { challengeId, options } = start.data;
  const attResp = await startRegistration({ optionsJSON: options });
  const verify = await API.post(
    "/passkey/register/verify",
    { challengeId, response: attResp },
    { headers: authHeaders(token) }
  );
  return verify.data;
};

// Sign in with a discoverable passkey (no username needed).
export const passkeySignin = async () => {
  const start = await API.post("/passkey/auth/options");
  const { challengeId, options } = start.data;
  const authResp = await startAuthentication({ optionsJSON: options });
  const verify = await API.post("/passkey/auth/verify", {
    challengeId,
    response: authResp,
  });
  return verify.data;
};
