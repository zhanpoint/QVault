import type { ResourceItem } from "../library/types";
export type AlbumTab = "albums" | "photos" | "videos" | "recycle";
export type AlbumSelection = { id: string; title: string };
export type RecyclePage = {
  items: ResourceItem[];
  nextOffset: number;
  complete: boolean;
  total: number | null;
};
