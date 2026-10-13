import type { ArchiveItem } from "../archive/types";

export type Contact = {
  uin: string;
  nickname: string;
  posts: number;
  dialoguePosts: number;
  firstAt: number;
  lastAt: number;
  lastMessage: { content: string; createdAt: number } | null;
};

export type ContactDirectory = {
  snapshotId: number;
  updatedAt: number;
  contacts: Contact[];
};

export type ContactFeedPage = { items: ArchiveItem[]; total: number };
