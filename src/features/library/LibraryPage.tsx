import { isTauri, invoke } from "@tauri-apps/api/core";
import {
  LoaderCircle,
  MessageCircle,
  Pause,
  RefreshCw,
  Search,
  Star,
  X,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "../../components/ui/button";
import { FavoriteCard, GuestbookCard } from "./ContentCards";
import { EmptyState, ErrorNotice, Pager } from "./LibraryUI";
import { errorMessage, PAGE_SIZE, useResources } from "./useResources";

type ContentModule = "guestbook" | "favorites";
type PageProps = {
  module: ContentModule;
  loggedIn: boolean;
  requireLogin: () => Promise<boolean>;
};

function Records({ module, loggedIn, requireLogin }: PageProps) {
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [year, setYear] = useState<number | null>(null);
  const [source, setSource] = useState<"current" | "history" | null>(null);
  const [years, setYears] = useState<number[]>([]);
  const [yearsError, setYearsError] = useState("");
  const [refresh, setRefresh] = useState(0);
  const resources = useResources(module, null, loggedIn, requireLogin, {
    query,
    year,
    source,
  });
  const { data, page, loading, syncing, message, error } = resources;
  const loadYears = useCallback(
    async () => invoke<number[]>("list_qzone_library_years", { module }),
    [module],
  );
  useEffect(() => {
    let cancelled = false;
    void loadYears()
      .then((result) => {
        if (!cancelled) {
          setYears(result);
          setYearsError("");
        }
      })
      .catch((reason) => {
        if (!cancelled) setYearsError(errorMessage(reason));
      });
    return () => {
      cancelled = true;
    };
  }, [loadYears, data?.syncedAt, refresh]);
  const reset = () => {
    setSearch("");
    setQuery("");
    setYear(null);
    setSource(null);
  };
  const filtered = Boolean(query || year || source);
  const Icon = module === "guestbook" ? MessageCircle : Star;
  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-border bg-surface px-5 py-4">
        <div>
          <p className="m-0 text-sm font-semibold">
            {data
              ? `${filtered ? "匹配" : "本地保存"} ${data.total} 条记录`
              : "正在读取本地记录"}
          </p>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {data?.syncedAt
              ? `${data.complete ? "同步完成" : "同步未完成，可继续"} · ${new Date(data.syncedAt * 1000).toLocaleString("zh-CN")}`
              : "同步会保存记录，已保存的内容可从本地浏览。"}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {syncing ? (
            <Button variant="outline" onClick={() => void resources.pause()}>
              <Pause size={15} />
              暂停同步
            </Button>
          ) : (
            <Button disabled={loading} onClick={() => void resources.sync()}>
              <RefreshCw size={15} />
              {error
                ? "重试同步"
                : data?.syncedAt && !data.complete
                  ? "继续同步"
                  : "同步 QQ 空间"}
            </Button>
          )}
          <Button
            variant="ghost"
            aria-label="刷新本地列表"
            disabled={loading || syncing}
            onClick={() => {
              void resources.load(page);
              setRefresh((value) => value + 1);
            }}
          >
            <RefreshCw size={16} />
          </Button>
        </div>
      </div>
      <form
        role="search"
        className="flex flex-wrap gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          setQuery(search.trim());
        }}
      >
        <div className="relative min-w-0 flex-1 basis-48">
          <Search
            size={16}
            aria-hidden="true"
            className="pointer-events-none absolute left-3 top-3 text-muted-foreground"
          />
          <input
            aria-label="搜索已保存的记录"
            value={search}
            disabled={syncing}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={
              module === "guestbook"
                ? "搜索留言内容、昵称或 QQ 号"
                : "搜索收藏标题、正文或作者"
            }
            className="h-10 w-full rounded-xl border border-border bg-surface pl-9 pr-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
          />
        </div>
        <Button
          type="submit"
          variant="outline"
          className="h-10"
          disabled={loading || syncing}
        >
          搜索
        </Button>
        <select
          aria-label="按年份筛选"
          disabled={syncing || !years.length}
          value={year ?? ""}
          onChange={(event) =>
            setYear(event.target.value ? Number(event.target.value) : null)
          }
          className="h-10 rounded-xl border border-border bg-surface px-3 text-sm"
        >
          <option value="">全部年份</option>
          {years.map((value) => (
            <option key={value} value={value}>
              {value} 年
            </option>
          ))}
        </select>
        {module === "guestbook" && (
          <select
            aria-label="按留言来源筛选"
            disabled={syncing}
            value={source ?? ""}
            onChange={(event) =>
              setSource(
                event.target.value === "current"
                  ? "current"
                  : event.target.value === "history"
                    ? "history"
                    : null,
              )
            }
            className="h-10 rounded-xl border border-border bg-surface px-3 text-sm"
          >
            <option value="">全部来源</option>
            <option value="current">当前留言板</option>
            <option value="history">历史留言残留</option>
          </select>
        )}
        {filtered && (
          <Button
            type="button"
            variant="ghost"
            className="h-10"
            disabled={syncing}
            onClick={reset}
          >
            <X size={14} />
            清除筛选
          </Button>
        )}
      </form>
      <ErrorNotice error={error} />
      {yearsError && (
        <p role="status" className="text-xs text-muted-foreground">
          年份筛选暂不可用：{yearsError}
        </p>
      )}
      {message && (
        <p
          role="status"
          className={`flex items-center gap-2 text-sm leading-6 ${syncing ? "rounded-xl bg-primary/5 px-4 py-3 text-primary" : "text-muted-foreground"}`}
        >
          {syncing && (
            <LoaderCircle
              size={16}
              className="shrink-0 motion-safe:animate-spin"
            />
          )}
          {message}
        </p>
      )}
      {loading ? (
        <div
          role="status"
          className="grid min-h-56 place-items-center text-sm text-muted-foreground"
        >
          正在读取记录…
        </div>
      ) : data?.items.length ? (
        <div
          key={`${page}-${query}-${year}-${source}`}
          className={
            module === "guestbook"
              ? "mx-auto max-w-3xl space-y-4"
              : "grid items-start gap-4 md:grid-cols-2"
          }
        >
          {data.items.map((item) =>
            module === "guestbook" ? (
              <GuestbookCard key={item.itemKey} item={item} />
            ) : (
              <FavoriteCard key={item.itemKey} item={item} />
            ),
          )}
        </div>
      ) : (
        <EmptyState
          icon={Icon}
          title={
            error
              ? "暂时无法读取记录"
              : filtered
                ? "没有找到匹配的记录"
                : data?.complete
                  ? "当前没有可展示的记录"
                  : "这些记忆还未同步"
          }
          description={
            error
              ? "重试不会清除已保存的记录和同步断点。"
              : filtered
                ? "尝试其他关键词，或清除筛选查看全部内容。"
                : module === "guestbook"
                  ? "同步 QQ 空间留言，或先完成归档以查看历史留言残留。"
                  : "点击同步，保存 QQ 空间收藏中的内容。"
          }
        >
          {filtered && (
            <Button variant="outline" onClick={reset}>
              查看全部
            </Button>
          )}
        </EmptyState>
      )}
      {data && (data.total > PAGE_SIZE || page > 0) && (
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

export function LibraryPage(props: PageProps) {
  const { module, loggedIn, requireLogin } = props;
  const guestbook = module === "guestbook";
  const Icon = guestbook ? MessageCircle : Star;
  return (
    <section className="mx-auto max-w-5xl space-y-5">
      <header className="flex items-center gap-3 py-1">
        <span className="grid size-11 shrink-0 place-items-center rounded-xl bg-primary/10 text-primary">
          <Icon size={23} />
        </span>
        <div>
          <h1 className="m-0 text-xl font-bold tracking-tight">
            {guestbook ? "留言板" : "收藏"}
          </h1>
          <p className="mt-1 text-sm leading-6 text-muted-foreground">
            {guestbook
              ? "留下的话，和当时回应它的人。"
              : "把舍不得忘记的内容，重新放回手边。"}
          </p>
        </div>
      </header>
      {!isTauri() ? (
        <EmptyState
          icon={Icon}
          title="连接本地服务后使用"
          description="当前浏览器未连接本地服务，暂时无法读取登录会话或 QQ 空间数据。"
        />
      ) : !loggedIn ? (
        <EmptyState
          icon={Icon}
          title={`登录后查看你的${guestbook ? "留言" : "收藏"}`}
          description="使用本人 QQ 账号登录，即可同步并浏览已保存的记录。"
        >
          <Button onClick={() => void requireLogin()}>登录 QQ 空间</Button>
        </EmptyState>
      ) : (
        <Records {...props} />
      )}
    </section>
  );
}
