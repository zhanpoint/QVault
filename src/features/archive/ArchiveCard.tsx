import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import {
  ChevronDown,
  ChevronUp,
  Heart,
  ImageOff,
  LoaderCircle,
  MessageCircle,
  Play,
  RotateCcw,
  X,
} from "lucide-react";
import { memo, useEffect, useRef, useState } from "react";
import { QzoneText } from "../../components/QzoneText";
import { Button } from "../../components/ui/button";
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from "../../components/ui/card";
import type { ArchiveItem } from "./types";

const dateTime = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
});
const formatTime = (timestamp: number) =>
  timestamp > 0 ? dateTime.format(new Date(timestamp * 1000)) : "时间未知";

function ImagePreview({
  source,
  alt,
  close,
}: {
  source: string;
  alt: string;
  close: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  return (
    <dialog
      ref={dialog}
      onClose={close}
      onCancel={close}
      aria-label={alt}
      className="archive-image-dialog"
      onClick={(event) => {
        if (event.target === event.currentTarget) dialog.current?.close();
      }}
    >
      <Button
        aria-label="关闭图片预览"
        variant="secondary"
        className="absolute right-4 top-4 z-10 px-2.5"
        onClick={() => dialog.current?.close()}
      >
        <X size={18} />
      </Button>
      <img
        src={source}
        alt={alt}
        className="max-h-[85vh] max-w-full rounded-lg object-contain"
      />
    </dialog>
  );
}

function ArchiveImage({
  id,
  index,
  url,
  alt,
  single,
}: {
  id: number;
  index: number;
  url: string;
  alt: string;
  single: boolean;
}) {
  const container = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [source, setSource] = useState("");
  const [error, setError] = useState("");
  const [preview, setPreview] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!container.current) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: "200px" },
    );
    observer.observe(container.current);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    setSource("");
    setError("");
    void invoke<string>("load_archived_image", {
      id,
      pictureIndex: index,
      forceReload: attempt > 0,
    })
      .then((path) => {
        if (!cancelled) {
          const src = convertFileSrc(path);
          setSource(
            attempt && !src.startsWith("data:")
              ? `${src}${src.includes("?") ? "&" : "?"}v=${attempt}`
              : src,
          );
        }
      })
      .catch((reason) => {
        if (!cancelled) setError(String(reason));
      });
    return () => {
      cancelled = true;
    };
  }, [id, index, url, attempt, visible]);

  return (
    <div
      ref={container}
      className={`${single ? "aspect-[4/3] max-h-96" : "aspect-square"} min-w-0 overflow-hidden rounded-xl bg-muted`}
    >
      {source ? (
        <button
          type="button"
          className="h-full w-full overflow-hidden"
          aria-label={`预览${alt}`}
          onClick={() => setPreview(true)}
        >
          <img
            src={source}
            alt={alt}
            className="h-full w-full object-cover"
            loading="lazy"
            decoding="async"
            onError={() => {
              setSource("");
              setError("图片文件无法解码，重试将重新下载");
            }}
          />
        </button>
      ) : (
        <div className="grid h-full place-items-center p-3 text-center text-xs text-muted-foreground">
          {error ? (
            <button
              title={error}
              type="button"
              className="grid gap-1 rounded-lg p-2 hover:text-foreground"
              onClick={() => setAttempt((value) => value + 1)}
            >
              <ImageOff size={22} className="mx-auto" />
              <span>图片暂不可用</span>
              <span className="inline-flex items-center justify-center gap-1 text-primary">
                <RotateCcw size={11} />
                重试
              </span>
            </button>
          ) : (
            <span className="grid justify-items-center gap-2">
              {visible ? (
                <LoaderCircle size={20} className="motion-safe:animate-spin" />
              ) : (
                <ImageOff size={20} />
              )}
              <span>{visible ? "正在读取图片" : "等待加载"}</span>
            </span>
          )}
        </div>
      )}
      {preview && source && (
        <ImagePreview
          source={source}
          alt={alt}
          close={() => setPreview(false)}
        />
      )}
    </div>
  );
}

function ArchiveVideo({ id }: { id: number }) {
  const [source, setSource] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const pending = useRef(false);
  const load = async () => {
    if (pending.current) return;
    pending.current = true;
    setLoading(true);
    setError("");
    try {
      setSource(
        convertFileSrc(await invoke<string>("load_archived_video", { id })),
      );
    } catch (reason) {
      setError(String(reason));
    } finally {
      pending.current = false;
      setLoading(false);
    }
  };
  if (source)
    return (
      <video
        controls
        playsInline
        preload="metadata"
        src={source}
        className="mt-3 max-h-96 w-full rounded-xl bg-black"
        onError={() => {
          setSource("");
          setError("视频暂时无法播放，请重试");
        }}
      />
    );
  return (
    <button
      type="button"
      disabled={loading}
      className="mt-3 grid min-h-44 w-full place-items-center rounded-xl border border-border bg-muted/50 p-5 text-sm text-muted-foreground hover:text-foreground"
      onClick={() => void load()}
    >
      <span className="grid justify-items-center gap-3">
        <span className="grid size-11 place-items-center rounded-full bg-surface text-primary">
          {loading ? (
            <LoaderCircle size={20} className="motion-safe:animate-spin" />
          ) : error ? (
            <RotateCcw size={20} />
          ) : (
            <Play size={20} fill="currentColor" />
          )}
        </span>
        <span>
          {loading
            ? "正在准备视频…"
            : error
              ? "视频加载失败，点击重试"
              : "播放归档视频"}
        </span>
        {error && <span className="max-w-sm break-words text-xs">{error}</span>}
      </span>
    </button>
  );
}

export const ArchiveCard = memo(function ArchiveCard({
  item,
}: {
  item: ArchiveItem;
}) {
  const [expanded, setExpanded] = useState(false);
  const [fullText, setFullText] = useState(false);
  const [allImages, setAllImages] = useState(false);
  const { comments, likes } = item;
  const author = item.authorName || item.authorUin || "QQ 空间用户";
  const images = allImages ? item.pictureUrls : item.pictureUrls.slice(0, 9);
  const long = (item.content?.length ?? 0) > 240;

  return (
    <Card className="archive-feed-card shadow-sm">
      <CardHeader>
        <span
          aria-hidden="true"
          className="grid size-10 shrink-0 place-items-center rounded-full bg-primary/10 text-sm font-semibold text-primary"
        >
          {Array.from(author)[0]}
        </span>
        <div className="min-w-0 flex-1">
          <strong className="block truncate text-sm font-semibold">
            {author}
          </strong>
          <time
            dateTime={
              item.publishedAt > 0
                ? new Date(item.publishedAt * 1000).toISOString()
                : undefined
            }
            className="mt-0.5 block text-xs text-muted-foreground"
          >
            {formatTime(item.publishedAt)}
          </time>
        </div>
        <span className="hidden items-center gap-1 rounded-md bg-muted px-2 py-1 text-[11px] text-muted-foreground sm:inline-flex">
          已归档
        </span>
      </CardHeader>
      <CardContent>
        <p
          className={`whitespace-pre-wrap break-words text-sm leading-7 ${long && !fullText ? "line-clamp-5" : ""}`}
        >
          <QzoneText text={item.content || "（原记录未保留文字正文）"} />
        </p>
        {long && (
          <button
            type="button"
            aria-expanded={fullText}
            className="mt-2 text-xs font-medium text-primary"
            onClick={() => setFullText((value) => !value)}
          >
            {fullText ? "收起正文" : "展开全文"}
          </button>
        )}
        {images.length > 0 && (
          <div
            className={`mt-4 grid gap-2 ${images.length === 1 ? "max-w-xl grid-cols-1" : images.length === 2 || images.length === 4 ? "grid-cols-2" : "grid-cols-2 sm:grid-cols-3"}`}
          >
            {images.map((url, index) => (
              <ArchiveImage
                key={`${item.id}-${index}`}
                id={item.id}
                index={index}
                url={url}
                single={images.length === 1}
                alt={`说说图片 ${index + 1}`}
              />
            ))}
          </div>
        )}
        {item.pictureUrls.length > 9 && (
          <button
            type="button"
            className="mt-3 text-xs text-primary"
            onClick={() => setAllImages((value) => !value)}
          >
            {allImages
              ? "收起图片"
              : `查看全部 ${item.pictureUrls.length} 张图片`}
          </button>
        )}
        {item.videoUrls.length > 0 && <ArchiveVideo id={item.id} />}
      </CardContent>
      <CardFooter>
        <span className="inline-flex items-center gap-1.5 px-2 text-xs text-muted-foreground">
          <Heart size={15} />
          {item.likeCount} 赞
        </span>
        <Button
          variant="ghost"
          className="h-8 text-xs font-normal"
          aria-expanded={expanded}
          aria-controls={`archive-comments-${item.id}`}
          onClick={() => setExpanded((value) => !value)}
        >
          <MessageCircle size={15} />
          {item.commentCount} 条评论
          {expanded ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
        </Button>
      </CardFooter>
      {expanded && (
        <div
          id={`archive-comments-${item.id}`}
          className="space-y-4 border-t border-border bg-muted/20 px-5 py-4 text-xs"
        >
          {likes.length > 0 && (
            <div className="flex items-start gap-2 leading-6 text-muted-foreground">
              <Heart size={14} className="mt-1.5 shrink-0 text-primary" />
              <p className="break-words">
                {likes
                  .map((like) => like.nickname || like.uin || "QQ 用户")
                  .join("、")}{" "}
                赞过这条说说
              </p>
            </div>
          )}
          {comments.length > 0 ? (
            comments.map((comment, index) => (
              <div
                key={`${comment.uin ?? "comment"}-${comment.createdAt}-${index}`}
                className="flex gap-2.5"
              >
                <span
                  aria-hidden="true"
                  className="grid size-7 shrink-0 place-items-center rounded-full bg-muted font-medium text-muted-foreground"
                >
                  {Array.from(comment.nickname || comment.uin || "访")[0]}
                </span>
                <div className="min-w-0 flex-1">
                  <p className="whitespace-pre-wrap break-words leading-6">
                    <strong className="font-semibold">
                      {comment.nickname || comment.uin || "访客"}
                    </strong>
                    <span className="mx-1 text-muted-foreground">·</span>
                    <QzoneText text={comment.content} />
                  </p>
                  {comment.createdAt > 0 && (
                    <time className="mt-0.5 block text-[11px] text-muted-foreground">
                      {formatTime(comment.createdAt)}
                    </time>
                  )}
                  {comment.replies.map((reply, replyIndex) => (
                    <p
                      key={`${reply.uin ?? "reply"}-${reply.createdAt}-${replyIndex}`}
                      className="mt-2 whitespace-pre-wrap break-words border-l-2 border-border pl-3 leading-6"
                    >
                      <strong>
                        {reply.nickname || reply.uin || "QQ 用户"}
                      </strong>
                      {reply.replyToNickname || reply.replyToUin
                        ? ` 回复 ${reply.replyToNickname || reply.replyToUin}`
                        : ""}
                      ：<QzoneText text={reply.content} />
                    </p>
                  ))}
                </div>
              </div>
            ))
          ) : (
            <p className="py-2 text-center text-muted-foreground">
              原记录中暂无可展示的评论正文
            </p>
          )}
        </div>
      )}
    </Card>
  );
});
