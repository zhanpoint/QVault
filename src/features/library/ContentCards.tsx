import {
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  History,
  LockKeyhole,
  MessageCircle,
  Music2,
  Star,
} from "lucide-react";
import { useId, useState } from "react";
import { QzoneText } from "../../components/QzoneText";
import { Button } from "../../components/ui/button";
import { MediaPreview, PlayBadge, ResourceImage } from "./ResourceMedia";
import type {
  ResourceImage as ImageItem,
  ResourceItem,
  ResourceVideo,
} from "./types";

function RecordDate({ timestamp }: { timestamp?: number }) {
  return timestamp && timestamp > 0 ? (
    <time dateTime={new Date(timestamp * 1000).toISOString()}>
      {new Date(timestamp * 1000).toLocaleString("zh-CN", {
        dateStyle: "medium",
        timeStyle: "short",
      })}
    </time>
  ) : null;
}

function ContentText({ text }: { text?: string }) {
  const [expanded, setExpanded] = useState(false);
  const id = useId();
  if (!text) return null;
  const long = text.length > 240 || text.split("\n").length > 6;
  return (
    <div>
      <p
        id={id}
        className={`m-0 whitespace-pre-wrap break-words text-sm leading-7 ${long && !expanded ? "line-clamp-6" : ""}`}
      >
        <QzoneText text={text} />
      </p>
      {long && (
        <Button
          variant="ghost"
          className="mt-1 -ml-3 text-xs"
          aria-expanded={expanded}
          aria-controls={id}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
          {expanded ? "收起正文" : "展开全文"}
        </Button>
      )}
    </div>
  );
}

function CopyLink({
  url,
  label = "复制原始链接",
}: {
  url: string;
  label?: string;
}) {
  const [copied, setCopied] = useState(false);
  const [failed, setFailed] = useState(false);
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (
    !["http:", "https:"].includes(parsed.protocol) ||
    parsed.username ||
    parsed.password
  )
    return null;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      setFailed(false);
    } catch {
      setFailed(true);
    }
  };
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-2 text-xs text-muted-foreground">
      <span className="min-w-0 truncate" title={url}>
        {parsed.hostname}
      </span>
      <Button
        variant="ghost"
        className="h-8 px-2 text-xs"
        onClick={() => void copy()}
      >
        {copied ? <Check size={13} /> : <Copy size={13} />}
        {copied ? "已复制" : label}
      </Button>
      {failed && (
        <span role="alert">
          无法复制，请选择地址：
          <span className="select-all break-all">{url}</span>
        </span>
      )}
    </div>
  );
}

function Attachments({
  images = [],
  videos = [],
}: {
  images?: ImageItem[];
  videos?: ResourceVideo[];
}) {
  const [expanded, setExpanded] = useState(false);
  const [preview, setPreview] = useState<{
    url: string;
    title: string;
    kind: "image" | "video";
  } | null>(null);
  if (!images.length && !videos.length) return null;
  const visibleImages = expanded ? images : images.slice(0, 4);
  return (
    <div className="space-y-3">
      {images.length > 0 && (
        <div className="grid grid-cols-2 gap-2">
          {visibleImages.map((image, index) => (
            <button
              key={image.url}
              type="button"
              aria-label={`查看原图 ${index + 1}`}
              className="overflow-hidden rounded-xl border border-border text-left"
              onClick={() =>
                setPreview({
                  url: image.url,
                  title: `原图 ${index + 1}`,
                  kind: "image",
                })
              }
            >
              <ResourceImage
                url={image.thumbnail}
                title={`图片 ${index + 1}`}
              />
            </button>
          ))}
        </div>
      )}
      {images.length > 4 && (
        <Button
          variant="ghost"
          className="text-xs"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? "收起图片" : `展开全部 ${images.length} 张图片`}
        </Button>
      )}
      {videos.map((video, index) => (
        <div key={`${video.url ?? video.title}-${index}`}>
          <button
            type="button"
            disabled={!video.url}
            aria-label={`播放：${video.title}`}
            className="relative block w-full overflow-hidden rounded-xl border border-border text-left"
            onClick={() =>
              video.url &&
              setPreview({ url: video.url, title: video.title, kind: "video" })
            }
          >
            <ResourceImage url={video.coverUrl} title={video.title} />
            {video.url && <PlayBadge />}
          </button>
          {!video.url && (
            <p className="text-xs text-muted-foreground">
              {video.title} · 接口未提供可播放地址
            </p>
          )}
        </div>
      ))}
      {preview && (
        <MediaPreview {...preview} onClose={() => setPreview(null)} />
      )}
    </div>
  );
}

export function GuestbookCard({ item }: { item: ResourceItem }) {
  const [repliesOpen, setRepliesOpen] = useState(false);
  const replyId = useId();
  if (!item.summary?.trim()) return null;
  const name =
    item.authorName || (item.authorUin ? `QQ ${item.authorUin}` : "空间留言");
  const replies = item.replies ?? [];
  return (
    <article className="library-content-card overflow-hidden rounded-2xl border border-border bg-surface shadow-sm">
      <div className="flex items-start gap-3 border-b border-border px-5 py-4">
        <span
          aria-hidden="true"
          className="grid size-10 shrink-0 place-items-center rounded-full bg-primary/10 text-sm font-semibold text-primary"
        >
          {Array.from(name)[0]}
        </span>
        <div className="min-w-0 flex-1">
          <h2 className="m-0 break-words text-sm font-semibold">{name}</h2>
          <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
            <RecordDate timestamp={item.createdAt} />
            {item.authorUin && <span>QQ {item.authorUin}</span>}
          </div>
        </div>
      </div>
      <div className="space-y-4 px-5 py-4">
        <div className="flex flex-wrap gap-2 text-xs">
          {item.source === "history" && (
            <span className="inline-flex items-center gap-1 rounded-md bg-muted px-2 py-1 text-muted-foreground">
              <History size={12} />
              历史留言残留
            </span>
          )}
          {item.privateMessage && (
            <span className="inline-flex items-center gap-1 rounded-md bg-primary/10 px-2 py-1 text-primary">
              <LockKeyhole size={12} />
              私密留言
            </span>
          )}
        </div>
        <ContentText text={item.summary} />
        <Attachments images={item.images} />
      </div>
      {replies.length > 0 && (
        <div className="border-t border-border px-5 py-3">
          <Button
            variant="ghost"
            className="-ml-3"
            aria-expanded={repliesOpen}
            aria-controls={replyId}
            onClick={() => setRepliesOpen((value) => !value)}
          >
            <MessageCircle size={15} />
            {repliesOpen ? "收起回复" : `查看 ${replies.length} 条回复`}
          </Button>
          {repliesOpen && (
            <ol
              id={replyId}
              className="mt-3 list-none space-y-4 border-l-2 border-primary/15 pl-4"
            >
              {replies.map((reply, index) => (
                <li key={index} className="space-y-2">
                  <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                    <span className="text-xs font-semibold">
                      {reply.authorName ||
                        (reply.authorUin ? `QQ ${reply.authorUin}` : "回复")}
                    </span>
                    <span className="text-xs text-muted-foreground">
                      <RecordDate timestamp={reply.createdAt} />
                    </span>
                  </div>
                  <ContentText text={reply.content} />
                  <Attachments images={reply.images} />
                </li>
              ))}
            </ol>
          )}
        </div>
      )}
    </article>
  );
}

export function FavoriteCard({ item }: { item: ResourceItem }) {
  return (
    <article className="library-content-card flex min-w-0 flex-col overflow-hidden rounded-2xl border border-border bg-surface shadow-sm">
      <div className="flex items-center gap-2 border-b border-border px-5 py-3 text-xs text-muted-foreground">
        <Star size={14} className="text-primary" />
        <span>{item.kind || "空间收藏"}</span>
        <span className="ml-auto">
          <RecordDate timestamp={item.createdAt} />
        </span>
      </div>
      <div className="flex-1 space-y-4 px-5 py-4">
        <h2 className="m-0 break-words text-base font-semibold leading-6">
          {item.title}
        </h2>
        {item.authorName && (
          <p className="m-0 text-xs text-muted-foreground">
            来源作者：{item.authorName}
          </p>
        )}
        <ContentText text={item.summary} />
        <Attachments images={item.images} videos={item.videos} />
        {item.audioUrls?.map((url) => (
          <div key={url} className="rounded-xl bg-muted/50 p-3">
            <span className="flex items-center gap-2 text-xs">
              <Music2 size={14} />
              音频原始地址
            </span>
            <CopyLink url={url} label="复制音频地址" />
          </div>
        ))}
      </div>
      {item.sourceUrl && (
        <div className="border-t border-border px-5 py-2">
          <CopyLink url={item.sourceUrl} />
        </div>
      )}
    </article>
  );
}
