/** Backend commands reject with a human-readable string; normalise anything else. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  if (e && typeof e === "object" && "message" in e && typeof e.message === "string") {
    return e.message;
  }
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}
