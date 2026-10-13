export type ArchiveProgress = {
  ownerUin: string | null;
  mode: "own" | "recovery";
  stage: "visible" | "history" | "interactions" | "finalizing" | "library";
  active: boolean;
  status: string;
  pages: number;
  fetched: number;
  saved: number;
  archived: number;
  skipped: number;
  message: string;
  retryAt: number | null;
  complete: boolean;
  canResume: boolean;
};

type ArchiveReply = {
  uin: string | null;
  nickname: string | null;
  replyToUin: string | null;
  replyToNickname: string | null;
  content: string;
  createdAt: number;
};

type ArchiveComment = {
  uin: string | null;
  nickname: string | null;
  content: string;
  createdAt: number;
  replies: ArchiveReply[];
};

type LikeUser = {
  uin: string | null;
  nickname: string | null;
  historical: boolean;
  likedAt: number;
};

export type ArchiveItem = {
  id: number;
  cellId: string;
  publishedAt: number;
  content: string | null;
  authorUin: string | null;
  authorName: string | null;
  pictureUrls: string[];
  videoUrls: string[];
  likeCount: number;
  commentCount: number;
  likes: LikeUser[];
  comments: ArchiveComment[];
};
