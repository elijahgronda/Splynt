import type { JumpBackInItem } from "../types";

const MAX_ITEMS = 12;

function storageKey(scope: string) {
  return `splice.jump-back-in.v1:${encodeURIComponent(scope)}`;
}

function isItem(value: unknown): value is JumpBackInItem {
  const item = value as Partial<JumpBackInItem> | null;
  return Boolean(
    item
      && (item.kind === "album" || item.kind === "artist" || item.kind === "playlist")
      && typeof item.id === "string"
      && typeof item.title === "string"
      && typeof item.subtitle === "string"
      && (item.coverArt === undefined || typeof item.coverArt === "string"),
  );
}

export function readRecentCollections(scope: string): JumpBackInItem[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(storageKey(scope)) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter(isItem).slice(0, MAX_ITEMS) : [];
  } catch {
    return [];
  }
}

export function rememberRecentCollection(scope: string, item: JumpBackInItem) {
  const next = [
    item,
    ...readRecentCollections(scope).filter((saved) => `${saved.kind}:${saved.id}` !== `${item.kind}:${item.id}`),
  ].slice(0, MAX_ITEMS);
  try {
    localStorage.setItem(storageKey(scope), JSON.stringify(next));
  } catch {
    // Navigation remains successful when the browser store is full.
  }
  return next;
}
