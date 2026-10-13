import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { ImageOff, LoaderCircle, Play, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "../../components/ui/button";
import { errorMessage } from "./useResources";

function resourceSrc(url: string, kind: "image" | "video") {
  return invoke<string>("load_library_resource", {
    resourceUrl: url,
    kind,
  }).then(convertFileSrc);
}

export function ResourceImage({
  url,
  title,
}: {
  url?: string | null;
  title: string;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [source, setSource] = useState("");
  const [error, setError] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setSource("");
    setError(false);
    if (!url || !host.current) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry.isIntersecting) return;
        observer.disconnect();
        void resourceSrc(url, "image")
          .then((src) => {
            if (!cancelled) setSource(src);
          })
          .catch(() => {
            if (!cancelled) setError(true);
          });
      },
      { rootMargin: "180px" },
    );
    observer.observe(host.current);
    return () => {
      cancelled = true;
      observer.disconnect();
    };
  }, [url]);
  return (
    <div
      ref={host}
      className="flex aspect-[4/3] items-center justify-center overflow-hidden bg-muted text-muted-foreground"
    >
      {source && !error ? (
        <img
          src={source}
          alt={title}
          decoding="async"
          className="size-full object-cover"
          onError={() => setError(true)}
        />
      ) : error ? (
        <span className="flex items-center gap-2 text-xs">
          <ImageOff size={18} />
          预览暂不可用
        </span>
      ) : url ? (
        <LoaderCircle
          size={20}
          className="motion-safe:animate-spin"
          aria-label="正在加载预览"
        />
      ) : (
        <ImageOff size={24} aria-label="暂无封面" />
      )}
    </div>
  );
}

export function MediaPreview({
  url,
  title,
  kind,
  onClose,
}: {
  url: string;
  title: string;
  kind: "image" | "video";
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [source, setSource] = useState("");
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const element = dialog.current!;
    element.showModal();
    return () => element.close();
  }, []);
  useEffect(() => {
    let cancelled = false;
    setSource("");
    setError("");
    void resourceSrc(url, kind)
      .then((src) => {
        if (!cancelled) setSource(src);
      })
      .catch((reason) => {
        if (!cancelled) setError(errorMessage(reason));
      });
    return () => {
      cancelled = true;
    };
  }, [url, kind, attempt]);
  return (
    <dialog
      ref={dialog}
      className="album-preview rounded-2xl border border-border bg-surface p-4 text-foreground shadow-2xl"
      aria-labelledby="album-preview-title"
      onCancel={onClose}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="mb-4 flex items-center justify-between gap-4">
        <h3
          id="album-preview-title"
          className="m-0 min-w-0 truncate text-sm font-semibold"
        >
          {title}
        </h3>
        <Button variant="ghost" aria-label="关闭预览" onClick={onClose}>
          <X size={18} />
        </Button>
      </div>
      {error ? (
        <div
          role="alert"
          className="grid min-h-52 place-content-center gap-4 p-6 text-center"
        >
          <p className="max-w-md break-words text-sm">{error}</p>
          <Button
            variant="outline"
            onClick={() => setAttempt((value) => value + 1)}
          >
            重新加载
          </Button>
        </div>
      ) : !source ? (
        <div
          role="status"
          className="flex min-h-52 items-center justify-center gap-3 text-sm text-muted-foreground"
        >
          <LoaderCircle size={20} className="motion-safe:animate-spin" />
          {kind === "video"
            ? "正在准备视频，较大文件需要更多时间…"
            : "正在加载原图…"}
        </div>
      ) : kind === "video" ? (
        <video
          src={source}
          controls
          autoPlay
          playsInline
          preload="none"
          className="max-h-[75vh] w-full rounded-xl bg-black"
          onError={() => setError("视频格式无法播放，请重新同步后重试。")}
        />
      ) : (
        <img
          src={source}
          alt={title}
          className="max-h-[75vh] w-full rounded-xl object-contain"
          onError={() => setError("照片无法显示，请重新同步后重试。")}
        />
      )}
    </dialog>
  );
}

export function PlayBadge() {
  return (
    <span className="absolute inset-0 grid place-items-center">
      <span className="grid size-11 place-items-center rounded-full bg-black/55 text-white shadow-sm">
        <Play size={20} fill="currentColor" />
      </span>
    </span>
  );
}
