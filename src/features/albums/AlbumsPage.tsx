import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  ArrowLeft,
  FolderOpen,
  Image,
  Images,
  LoaderCircle,
  LockKeyhole,
  Pause,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Video,
} from "lucide-react";
import { motion, useReducedMotion } from "framer-motion";
import { useEffect, useRef, useState } from "react";
import { EmptyState, ErrorNotice, Pager } from "../library/LibraryUI";
import { Button } from "../../components/ui/button";
import {
  MediaPreview,
  PlayBadge,
  ResourceImage,
} from "../library/ResourceMedia";
import type { AlbumSelection, AlbumTab, RecyclePage } from "./types";
import type { ResourceItem } from "../library/types";
import { errorMessage, PAGE_SIZE, useResources } from "../library/useResources";

const tabs = [
  { id: "albums", label: "相册", icon: FolderOpen },
  { id: "photos", label: "照片", icon: Image },
  { id: "videos", label: "视频", icon: Video },
  { id: "recycle", label: "回收站", icon: Trash2 },
] as const;

function ResourceGrid({
  items,
  albumMode,
  videoMode,
  onAlbum,
}: {
  items: ResourceItem[];
  albumMode: boolean;
  videoMode?: boolean;
  onAlbum: (album: AlbumSelection) => void;
}) {
  const [preview, setPreview] = useState<{
    url: string;
    title: string;
    kind: "image" | "video";
  } | null>(null);
  return (
    <>
      <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-3 xl:grid-cols-4">
        {items.map((item) => {
          const playable = item.mediaUrls[0];
          const date = item.createdAt
            ? new Date(item.createdAt * 1000).toLocaleDateString("zh-CN")
            : "";
          const deleted = item.deletedAt
            ? /^\d{10}$/.test(item.deletedAt)
              ? new Date(Number(item.deletedAt) * 1000).toLocaleDateString(
                  "zh-CN",
                )
              : item.deletedAt
            : "";
          return (
            <article
              key={item.itemKey}
              className="album-resource-card group min-w-0 overflow-hidden rounded-2xl border border-border bg-surface shadow-sm motion-safe:transition-shadow hover:shadow-md"
            >
              <button
                type="button"
                className="relative block w-full text-left disabled:cursor-default"
                disabled={!albumMode && !playable}
                aria-label={
                  albumMode
                    ? `打开相册：${item.title}`
                    : `${videoMode ? "播放视频" : "查看原图"}：${item.title}`
                }
                onClick={() =>
                  albumMode
                    ? onAlbum({ id: item.itemKey, title: item.title })
                    : playable &&
                      setPreview({
                        url: playable,
                        title: item.title,
                        kind: videoMode ? "video" : "image",
                      })
                }
              >
                <ResourceImage url={item.coverUrl} title={item.title} />
                {videoMode && playable && <PlayBadge />}
              </button>
              <div className="space-y-1.5 px-3 py-3 sm:px-4">
                <h3
                  className="m-0 truncate text-sm font-semibold"
                  title={item.title}
                >
                  {item.title}
                </h3>
                {item.summary && item.summary !== item.title && (
                  <p className="m-0 line-clamp-2 text-xs leading-5 text-muted-foreground">
                    {item.summary}
                  </p>
                )}
                <p className="m-0 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
                  {albumMode ? (
                    <>
                      <FolderOpen size={13} />
                      {item.count != null
                        ? `${item.count} 张照片`
                        : "点击查看照片"}
                    </>
                  ) : videoMode && !playable ? (
                    "该视频暂无可播放地址"
                  ) : deleted ? (
                    `删除于 ${deleted}`
                  ) : (
                    date || (videoMode ? "点击播放" : "点击查看原图")
                  )}
                </p>
              </div>
            </article>
          );
        })}
      </div>
      {preview && (
        <MediaPreview {...preview} onClose={() => setPreview(null)} />
      )}
    </>
  );
}

function LibraryResources({
  module,
  album,
  loggedIn,
  requireLogin,
  onAlbum,
  onBack,
}: {
  module: "albums" | "photos" | "videos";
  album: AlbumSelection | null;
  loggedIn: boolean;
  requireLogin: () => Promise<boolean>;
  onAlbum: (album: AlbumSelection) => void;
  onBack: () => void;
}) {
  const resources = useResources(
    module,
    module === "photos" ? album!.id : null,
    loggedIn,
    requireLogin,
  );
  const { data, loading, syncing, error, message, page } = resources;
  const items = data?.items ?? [];
  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          {module === "photos" && (
            <Button variant="ghost" className="mb-2 -ml-3" onClick={onBack}>
              <ArrowLeft size={16} />
              选择其他相册
            </Button>
          )}
          <h2 className="m-0 text-base font-semibold">
            {module === "photos"
              ? album!.title
              : module === "albums"
                ? "你的相册"
                : "空间视频"}
          </h2>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {data?.syncedAt
              ? `本地已保存 ${data.total} 条 · ${data.complete ? "同步完成" : "可继续同步"} · ${new Date(data.syncedAt * 1000).toLocaleString("zh-CN")}`
              : "同步后，可从本地浏览已取得的记录"}
          </p>
        </div>
        <div className="flex shrink-0 gap-2">
          {syncing ? (
            <Button variant="outline" onClick={() => void resources.pause()}>
              <Pause size={15} />
              暂停同步
            </Button>
          ) : (
            <Button disabled={loading} onClick={() => void resources.sync()}>
              <RefreshCw size={15} />
              {data && !data.complete ? "继续同步" : "同步 QQ 空间"}
            </Button>
          )}
          <Button
            variant="ghost"
            aria-label="刷新本地列表"
            disabled={loading || syncing}
            onClick={() => void resources.load(page)}
          >
            <RefreshCw size={16} />
          </Button>
        </div>
      </div>
      <ErrorNotice error={error} />
      {syncing && (
        <p
          role="status"
          className="flex items-center gap-2 rounded-xl bg-primary/5 px-4 py-3 text-sm text-primary"
        >
          <LoaderCircle
            size={16}
            className="shrink-0 motion-safe:animate-spin"
          />
          {message}
        </p>
      )}
      {!syncing && message && (
        <p role="status" className="text-sm leading-6 text-muted-foreground">
          {message}
        </p>
      )}
      {loading ? (
        <div
          role="status"
          className="grid min-h-56 place-items-center text-sm text-muted-foreground"
        >
          正在读取本地记录…
        </div>
      ) : items.length ? (
        <ResourceGrid
          key={page}
          items={items}
          albumMode={module === "albums"}
          videoMode={module === "videos"}
          onAlbum={onAlbum}
        />
      ) : (
        <EmptyState
          title={
            error
              ? "暂时无法读取记录"
              : data?.complete
                ? "这里还没有内容"
                : module === "albums"
                  ? "让相册回到手边"
                  : "尚未同步这份记忆"
          }
          description={
            error
              ? "可以重试同步，已保存的记录和断点会保留。"
              : module === "albums"
                ? "同步 QQ 空间相册，然后打开相册查看其中的照片。"
                : "点击同步获取当前账号的记录。"
          }
        />
      )}
      {data && data.total > PAGE_SIZE && (
        <Pager
          page={page}
          loading={loading || syncing}
          more={(page + 1) * PAGE_SIZE < data.total}
          onPrevious={() => void resources.load(page - 1)}
          onNext={() => void resources.load(page + 1)}
        />
      )}
    </div>
  );
}

function RecycleResources() {
  const [verified, setVerified] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [kind, setKind] = useState<"albums" | "photos">("photos");
  const [album, setAlbum] = useState<AlbumSelection | null>(null);
  const [data, setData] = useState<RecyclePage | null>(null);
  const [offsets, setOffsets] = useState([0]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [reload, setReload] = useState(0);
  const verification = useRef(0);
  const alive = useRef(true);
  const offset = offsets.at(-1)!;

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      verification.current++;
      void invoke("close_recycle_password_window").catch(() => undefined);
    };
  }, []);

  useEffect(() => {
    if (!verified) return;
    let cancelled = false;
    setLoading(true);
    setError("");
    setData(null);
    void invoke<RecyclePage>("list_recycle_resources", {
      kind,
      offset,
      albumId: album?.id ?? null,
    })
      .then((result) => {
        if (!cancelled) setData(result);
      })
      .catch((reason) => {
        if (!cancelled) setError(errorMessage(reason));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [verified, kind, offset, album?.id, reload]);

  const verify = async () => {
    const run = ++verification.current;
    setVerifying(true);
    setError("");
    setVerified(false);
    try {
      await invoke("open_recycle_password_window");
      while (alive.current && verification.current === run) {
        const ready = await invoke<boolean>("check_recycle_password");
        if (!alive.current || verification.current !== run) return;
        if (ready) {
          await invoke("close_recycle_password_window");
          if (!alive.current || verification.current !== run) return;
          setOffsets([0]);
          setVerified(true);
          setVerifying(false);
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 1200));
      }
    } catch (reason) {
      if (alive.current && verification.current === run)
        setError(errorMessage(reason));
    } finally {
      if (alive.current && verification.current === run) setVerifying(false);
    }
  };
  const cancelVerify = () => {
    verification.current++;
    setVerifying(false);
    void invoke("close_recycle_password_window").catch((reason) => {
      if (alive.current) setError(errorMessage(reason));
    });
  };
  const switchKind = (next: "albums" | "photos") => {
    setKind(next);
    setAlbum(null);
    setOffsets([0]);
  };
  return (
    <div className="space-y-5">
      <ErrorNotice error={error} />
      {!verified ? (
        <div className="grid min-h-72 place-content-center rounded-2xl border border-border bg-surface p-6 text-center">
          <LockKeyhole size={32} className="mx-auto mb-4 text-primary" />
          <h2 className="m-0 text-lg font-semibold">先验证 QQ 空间独立密码</h2>
          <p className="mx-auto mt-2 max-w-md text-sm leading-6 text-muted-foreground">
            在 QQ
            空间官方窗口完成验证后，就能查看仍在保留期内的已删除相册和照片。临时签名仅保存在当前登录会话中。
          </p>
          <div className="mt-5 flex justify-center gap-2">
            <Button disabled={verifying} onClick={() => void verify()}>
              <ShieldCheck size={16} />
              {verifying ? "等待官方窗口验证…" : "验证并查看"}
            </Button>
            {verifying && (
              <Button variant="outline" onClick={cancelVerify}>
                取消
              </Button>
            )}
          </div>
        </div>
      ) : (
        <>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div
              className="flex gap-1 rounded-xl bg-muted p-1"
              role="group"
              aria-label="回收站内容类型"
            >
              {(["photos", "albums"] as const).map((value) => (
                <Button
                  key={value}
                  variant={kind === value ? "outline" : "ghost"}
                  aria-pressed={kind === value}
                  disabled={loading}
                  onClick={() => switchKind(value)}
                >
                  {value === "albums" ? "已删除相册" : "已删除照片"}
                </Button>
              ))}
            </div>
            <div className="flex gap-1">
              <Button
                variant="ghost"
                disabled={loading}
                onClick={() => void verify()}
              >
                重新验证
              </Button>
              <Button
                variant="outline"
                disabled={loading}
                onClick={() => setReload((value) => value + 1)}
              >
                <RefreshCw size={15} />
                {error ? "重试当前页" : "刷新"}
              </Button>
            </div>
          </div>
          {album && (
            <div className="flex flex-wrap items-center gap-2">
              <Button variant="ghost" onClick={() => switchKind("albums")}>
                <ArrowLeft size={16} />
                返回回收站相册
              </Button>
              <span className="text-sm font-semibold">{album.title}</span>
            </div>
          )}
          <p className="text-xs leading-5 text-muted-foreground">
            {data?.total != null ? `QQ 空间报告 ${data.total} 条 · ` : ""}仅展示
            QQ 空间回收站当前可访问的内容。
          </p>
          {loading ? (
            <div
              role="status"
              className="grid min-h-64 place-items-center text-sm text-muted-foreground"
            >
              正在读取 QQ 空间回收站…
            </div>
          ) : data?.items.length ? (
            <ResourceGrid
              key={`${kind}-${album?.id}-${offset}`}
              items={data.items}
              albumMode={kind === "albums"}
              onAlbum={(next) => {
                setAlbum(next);
                setKind("photos");
                setOffsets([0]);
              }}
            />
          ) : (
            <EmptyState
              title={error ? "回收站暂不可用" : "当前页没有可展示的内容"}
              description={
                error
                  ? "可重新验证独立密码，或点击刷新重试。"
                  : "QQ 空间已清理或过了保留期的内容可能无法取得。"
              }
            />
          )}
          {(offsets.length > 1 || (data && !data.complete)) && (
            <Pager
              page={offsets.length - 1}
              loading={loading}
              more={Boolean(data && !data.complete)}
              onPrevious={() => setOffsets((value) => value.slice(0, -1))}
              onNext={() => {
                if (data) setOffsets((value) => [...value, data.nextOffset]);
              }}
            />
          )}
        </>
      )}
    </div>
  );
}

export function AlbumsPage({
  loggedIn,
  requireLogin,
}: {
  loggedIn: boolean;
  requireLogin: () => Promise<boolean>;
}) {
  const [tab, setTab] = useState<AlbumTab>("albums");
  const [album, setAlbum] = useState<AlbumSelection | null>(null);
  const reducedMotion = useReducedMotion();
  const tabList = useRef<HTMLDivElement>(null);
  const selectAlbum = (next: AlbumSelection) => {
    setAlbum(next);
    setTab("photos");
  };
  const selectTab = (next: AlbumTab) => setTab(next);
  return (
    <section className="mx-auto max-w-6xl space-y-5">
      <header className="flex items-center gap-3 py-1">
        <span className="grid size-11 place-items-center rounded-xl bg-primary/10 text-primary">
          <Images size={23} />
        </span>
        <div>
          <h1 className="m-0 text-xl font-bold tracking-tight">相册</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            照片、视频与曾经删掉的记忆，都在这里。
          </p>
        </div>
      </header>
      <div
        ref={tabList}
        role="tablist"
        aria-label="相册内容"
        className="flex gap-1 rounded-2xl border border-border bg-surface p-1.5"
        onKeyDown={(event) => {
          const current = tabs.findIndex((item) => item.id === tab);
          const next =
            event.key === "ArrowRight"
              ? (current + 1) % tabs.length
              : event.key === "ArrowLeft"
                ? (current + tabs.length - 1) % tabs.length
                : event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? tabs.length - 1
                    : -1;
          if (next < 0) return;
          event.preventDefault();
          selectTab(tabs[next].id);
          tabList.current
            ?.querySelectorAll<HTMLButtonElement>("[role=tab]")
            [next].focus();
        }}
      >
        {tabs.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            id={`album-tab-${id}`}
            type="button"
            role="tab"
            aria-selected={tab === id}
            aria-controls="album-panel"
            tabIndex={tab === id ? 0 : -1}
            onClick={() => selectTab(id)}
            className={`relative flex min-w-0 flex-1 items-center justify-center gap-1 rounded-xl px-1 py-3 text-xs font-medium sm:gap-2 sm:px-2 sm:text-sm ${tab === id ? "text-primary" : "text-muted-foreground hover:text-foreground"}`}
          >
            {tab === id && (
              <motion.span
                layoutId="album-active-tab"
                aria-hidden="true"
                className="absolute inset-0 rounded-xl bg-primary/10"
                transition={{ duration: reducedMotion ? 0 : 0.18 }}
              />
            )}
            <Icon size={16} className="relative shrink-0" />
            <span className="relative whitespace-nowrap">{label}</span>
          </button>
        ))}
      </div>
      <div
        id="album-panel"
        role="tabpanel"
        aria-labelledby={`album-tab-${tab}`}
        tabIndex={0}
        className="rounded-2xl focus-visible:outline-primary"
      >
        {!isTauri() ? (
          <EmptyState
            title="连接本地服务后使用"
            description="当前浏览器未连接本地服务，暂时无法读取登录会话或 QQ 空间数据。"
          />
        ) : !loggedIn ? (
          <EmptyState
            title="登录后查看你的相册"
            description="使用本人 QQ 账号登录，即可同步相册和视频，或验证独立密码后查看回收站。"
          >
            <Button onClick={() => void requireLogin()}>登录 QQ 空间</Button>
          </EmptyState>
        ) : tab === "recycle" ? (
          <RecycleResources />
        ) : (
          <LibraryResources
            key={`${tab}-${album?.id ?? "all"}`}
            module={tab === "photos" && !album ? "albums" : tab}
            album={album}
            loggedIn={loggedIn}
            requireLogin={requireLogin}
            onAlbum={selectAlbum}
            onBack={() => setAlbum(null)}
          />
        )}
      </div>
    </section>
  );
}
