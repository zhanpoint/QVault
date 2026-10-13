export type LibraryModule =
  "albums" | "photos" | "videos" | "guestbook" | "favorites";
export type ResourceItem = {
  itemKey: string;
  parentKey: string;
  title: string;
  summary?: string;
  createdAt?: number;
  coverUrl?: string | null;
  mediaUrls: string[];
  deletedAt?: string;
  count?: number | null;
  source?: "current" | "history";
  authorUin?: string | null;
  authorName?: string | null;
  kind?: string | null;
  sourceUrl?: string | null;
  privateMessage?: boolean;
  images?: ResourceImage[];
  videos?: ResourceVideo[];
  audioUrls?: string[];
  replies?: ResourceReply[];
};
export type LibraryPage = {
  items: ResourceItem[];
  total: number;
  remoteTotal: number;
  complete: boolean;
  syncedAt: number;
  lastError?: string | null;
};
export type ResourceImage = { url: string; thumbnail: string };
export type ResourceVideo = {
  url: string | null;
  coverUrl: string | null;
  title: string;
};
type ResourceReply = {
  authorUin: string | null;
  authorName: string | null;
  createdAt: number;
  content: string;
  images: ResourceImage[];
};
