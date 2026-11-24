// Account server for MSFS Flight Tracker: email and password, a session token,
// and one logbook per pilot. No third parties, no email provider: everything
// lives in the site's own D1 database.

const API_PREFIX = "/api/";
const SESSION_DAYS = 180;
// Workers cap PBKDF2 at 100000 iterations.
const PBKDF2_ROUNDS = 100_000;
const MAX_BODY = 4 * 1024 * 1024;
const MAX_FLIGHTS = 5000;
const ATTEMPT_WINDOW_MS = 5 * 60 * 1000;
const ATTEMPT_LIMIT = 12;

const now = () => Math.floor(Date.now() / 1000);

function json(data, status = 200, headers = {}) {
  return new Response(JSON.stringify(data), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
      ...headers,
    },
  });
}

function fail(code, message, status = 400, headers = {}) {
  return json({ error: code, message }, status, headers);
}

function hex(buffer) {
  return [...new Uint8Array(buffer)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function randomHex(bytes) {
  return hex(crypto.getRandomValues(new Uint8Array(bytes)));
}

async function passwordHash(password, saltHex) {
  const salt = new Uint8Array(saltHex.match(/../g).map((pair) => parseInt(pair, 16)));
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(password),
    "PBKDF2",
    false,
    ["deriveBits"],
  );
  const bits = await crypto.subtle.deriveBits(
    { name: "PBKDF2", hash: "SHA-256", salt, iterations: PBKDF2_ROUNDS },
    key,
    256,
  );
  return hex(bits);
}

async function tokenHash(token) {
  return hex(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token)));
}

function sameString(a, b) {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i += 1) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

const cleanEmail = (value) => String(value || "").trim().toLowerCase();
const validEmail = (value) => /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/.test(value) && value.length <= 190;

/* ---------- database ---------- */

let schemaReady = null;
function schema(env) {
  if (!schemaReady) {
    schemaReady = env.DB.batch([
      env.DB.prepare(
        `CREATE TABLE IF NOT EXISTS users (
           id TEXT PRIMARY KEY,
           email TEXT NOT NULL UNIQUE,
           salt TEXT NOT NULL,
           hash TEXT NOT NULL,
           created_at INTEGER NOT NULL
         )`,
      ),
      env.DB.prepare(
        `CREATE TABLE IF NOT EXISTS sessions (
           token TEXT PRIMARY KEY,
           user_id TEXT NOT NULL,
           created_at INTEGER NOT NULL,
           expires_at INTEGER NOT NULL
         )`,
      ),
      env.DB.prepare(
        `CREATE TABLE IF NOT EXISTS logbooks (
           user_id TEXT PRIMARY KEY,
           payload TEXT NOT NULL,
           updated_at INTEGER NOT NULL
         )`,
      ),
      env.DB.prepare("CREATE INDEX IF NOT EXISTS sessions_user ON sessions(user_id)"),
    ]).catch((error) => {
      schemaReady = null;
      throw error;
    });
  }
  return schemaReady;
}

async function createSession(env, userId) {
  const token = randomHex(32);
  const created = now();
  await env.DB.prepare(
    "INSERT INTO sessions (token, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
  )
    .bind(await tokenHash(token), userId, created, created + SESSION_DAYS * 86_400)
    .run();
  return token;
}

async function currentUser(request, env) {
  const header = request.headers.get("Authorization") || "";
  const token = header.startsWith("Bearer ") ? header.slice(7).trim() : "";
  if (!token) return null;
  const row = await env.DB.prepare(
    "SELECT u.id AS id, u.email AS email, s.expires_at AS expires_at FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token = ?",
  )
    .bind(await tokenHash(token))
    .first();
  if (!row) return null;
  if (row.expires_at < now()) return null;
  return { id: row.id, email: row.email };
}

/* ---------- request helpers ---------- */

const attempts = new Map();
function tooManyAttempts(key) {
  const stamp = Date.now();
  const list = (attempts.get(key) || []).filter((at) => stamp - at < ATTEMPT_WINDOW_MS);
  list.push(stamp);
  attempts.set(key, list);
  if (attempts.size > 2000) attempts.clear();
  return list.length > ATTEMPT_LIMIT;
}

async function readJson(request) {
  const text = await request.text();
  if (text.length > MAX_BODY) throw new Error("Payload too large");
  if (!text.trim()) return {};
  return JSON.parse(text);
}

const ALLOWED_ORIGINS = [
  "https://federicofnn.github.io",
  "https://msfs-flight-tracker-account-1vxd6r.freebuff.site",
];

function cors(request) {
  const origin = request.headers.get("Origin");
  if (!origin) return {};
  const local = /^https?:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/.test(origin);
  if (!local && !ALLOWED_ORIGINS.includes(origin)) return { Vary: "Origin" };
  return {
    "Access-Control-Allow-Origin": origin,
    "Access-Control-Allow-Headers": "authorization, content-type",
    "Access-Control-Allow-Methods": "GET, POST, PUT, DELETE, OPTIONS",
    "Access-Control-Max-Age": "86400",
    Vary: "Origin",
  };
}

/* ---------- API ---------- */

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const headers = cors(request);
    if (request.method === "OPTIONS") return new Response(null, { status: 204, headers });
    if (!url.pathname.startsWith(API_PREFIX)) {
      return fail("not_found", "This server only answers /api requests", 404, headers);
    }
    try {
      await schema(env);
    } catch (error) {
      return fail("server_error", `Database unavailable: ${error.message || error}`, 500, headers);
    }

    const route = url.pathname.slice(API_PREFIX.length).replace(/\/+$/, "");
    const method = request.method;
    const ip = request.headers.get("CF-Connecting-IP") || "local";

    try {
      if (route === "health") {
        return json({ ok: true, time: now() }, 200, headers);
      }

      if (route === "signup" && method === "POST") {
        if (tooManyAttempts(`signup:${ip}`)) {
          return fail("too_many", "Too many attempts: wait a few minutes", 429, headers);
        }
        let body;
        try {
          body = await readJson(request);
        } catch {
          return fail("bad_request", "The request is not valid JSON", 400, headers);
        }
        const email = cleanEmail(body.email);
        const password = String(body.password || "");
        if (!validEmail(email)) {
          return fail("bad_email", "Enter a valid email address", 400, headers);
        }
        if (password.length < 8) {
          return fail("short_password", "The password needs at least 8 characters", 400, headers);
        }
        if (password.length > 200) {
          return fail("long_password", "The password is too long", 400, headers);
        }
        const existing = await env.DB.prepare("SELECT id FROM users WHERE email = ?")
          .bind(email)
          .first();
        if (existing) {
          return fail("email_taken", "An account with this email already exists", 409, headers);
        }
        const id = randomHex(12);
        const salt = randomHex(16);
        await env.DB.prepare(
          "INSERT INTO users (id, email, salt, hash, created_at) VALUES (?, ?, ?, ?, ?)",
        )
          .bind(id, email, salt, await passwordHash(password, salt), now())
          .run();
        return json({ token: await createSession(env, id), email }, 200, headers);
      }

      if (route === "login" && method === "POST") {
        if (tooManyAttempts(`login:${ip}`)) {
          return fail("too_many", "Too many attempts: wait a few minutes", 429, headers);
        }
        let body;
        try {
          body = await readJson(request);
        } catch {
          return fail("bad_request", "The request is not valid JSON", 400, headers);
        }
        const email = cleanEmail(body.email);
        const password = String(body.password || "");
        const user = await env.DB.prepare("SELECT id, email, salt, hash FROM users WHERE email = ?")
          .bind(email)
          .first();
        // Same answer for a wrong email and a wrong password: no account probing.
        if (!user || !sameString(await passwordHash(password, user.salt), user.hash)) {
          return fail("bad_credentials", "Email or password is wrong", 401, headers);
        }
        return json({ token: await createSession(env, user.id), email: user.email }, 200, headers);
      }

      const user = await currentUser(request, env);
      if (!user) return fail("unauthorized", "Sign in first", 401, headers);

      if (route === "me" && method === "GET") {
        return json({ email: user.email }, 200, headers);
      }

      if (route === "logout" && method === "POST") {
        const header = request.headers.get("Authorization") || "";
        const token = header.startsWith("Bearer ") ? header.slice(7).trim() : "";
        await env.DB.prepare("DELETE FROM sessions WHERE token = ?")
          .bind(await tokenHash(token))
          .run();
        return json({ ok: true }, 200, headers);
      }

      if (route === "password" && method === "POST") {
        let body;
        try {
          body = await readJson(request);
        } catch {
          return fail("bad_request", "The request is not valid JSON", 400, headers);
        }
        const current = String(body.current || "");
        const next = String(body.next || "");
        if (next.length < 8) {
          return fail("short_password", "The password needs at least 8 characters", 400, headers);
        }
        const row = await env.DB.prepare("SELECT salt, hash FROM users WHERE id = ?")
          .bind(user.id)
          .first();
        if (!row || !sameString(await passwordHash(current, row.salt), row.hash)) {
          return fail("bad_credentials", "The current password is wrong", 401, headers);
        }
        const salt = randomHex(16);
        await env.DB.prepare("UPDATE users SET salt = ?, hash = ? WHERE id = ?")
          .bind(salt, await passwordHash(next, salt), user.id)
          .run();
        // Every other device has to sign in again.
        const header = request.headers.get("Authorization") || "";
        const token = header.startsWith("Bearer ") ? header.slice(7).trim() : "";
        await env.DB.prepare("DELETE FROM sessions WHERE user_id = ? AND token != ?")
          .bind(user.id, await tokenHash(token))
          .run();
        return json({ ok: true }, 200, headers);
      }

      if (route === "account" && method === "DELETE") {
        await env.DB.batch([
          env.DB.prepare("DELETE FROM logbooks WHERE user_id = ?").bind(user.id),
          env.DB.prepare("DELETE FROM sessions WHERE user_id = ?").bind(user.id),
          env.DB.prepare("DELETE FROM users WHERE id = ?").bind(user.id),
        ]);
        return json({ ok: true }, 200, headers);
      }

      if (route === "logbook" && method === "GET") {
        const row = await env.DB.prepare(
          "SELECT payload, updated_at FROM logbooks WHERE user_id = ?",
        )
          .bind(user.id)
          .first();
        if (!row) return json({ flights: [], live: null, updated: 0 }, 200, headers);
        let stored;
        try {
          stored = JSON.parse(row.payload);
        } catch {
          stored = { flights: [] };
        }
        return json(
          {
            flights: stored.flights || [],
            live: stored.live || null,
            updated: row.updated_at,
            version: stored.version || "",
          },
          200,
          headers,
        );
      }

      if (route === "logbook" && method === "PUT") {
        let body;
        try {
          body = await readJson(request);
        } catch {
          return fail("bad_request", "The logbook is not valid JSON", 400, headers);
        }
        if (!Array.isArray(body.flights)) {
          return fail("bad_request", "flights must be a list", 400, headers);
        }
        if (body.flights.length > MAX_FLIGHTS) {
          return fail("too_large", "Too many flights in one logbook", 413, headers);
        }
        const payload = JSON.stringify({
          app: "MSFS Flight Tracker",
          version: String(body.version || ""),
          updated: now(),
          flights: body.flights,
          live: body.live || null,
        });
        if (payload.length > MAX_BODY) {
          return fail("too_large", "The logbook is too large", 413, headers);
        }
        await env.DB.prepare(
          `INSERT INTO logbooks (user_id, payload, updated_at) VALUES (?, ?, ?)
           ON CONFLICT(user_id) DO UPDATE SET payload = excluded.payload, updated_at = excluded.updated_at`,
        )
          .bind(user.id, payload, now())
          .run();
        return json({ ok: true, updated: now(), flights: body.flights.length }, 200, headers);
      }

      return fail("not_found", "Unknown route", 404, headers);
    } catch (error) {
      return fail("server_error", String(error && error.message ? error.message : error), 500, headers);
    }
  },
};
