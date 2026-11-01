import { useState } from "react";
import {
  Archive,
  ArrowDown,
  Check,
  CircleAlert,
  Clock3,
  Database,
  LoaderCircle,
  Pause,
  Play,
  RefreshCw,
  Search,
  ShieldCheck,
  X,
} from "lucide-react";
import { Button } from "../../components/ui/button";
import { ArchiveCard } from "./ArchiveCard";
import { ArchiveRetries } from "./ArchiveRetries";
import { useArchive } from "./useArchive";

const statuses: Record<string, string> = {
  idle: "未开始",
  running: "归档中",
  pausing: "正在暂停",
  paused: "已暂停",
  abandoning: "正在结束",
  abandoned: "已放弃",
  error: "需重试",
  auth_required: "需要登录",
  incomplete: "待补充",
  limited: "冷却中",
  completed: "已完成",
};

export function ArchivePage({
  loggedIn,
  requireLogin,
}: {
  loggedIn: boolean;
  requireLogin: () => Promise<boolean>;
}) {
  const archive = useArchive(loggedIn, requireLogin);
  const [confirmAbandon, setConfirmAbandon] = useState(false);
  const { progress, items, busy, loading, remaining } = archive;
  const active = Boolean(progress?.active);
  const stopping =
    progress?.status === "pausing" || progress?.status === "abandoning";
  const status = statuses[progress?.status ?? "idle"] ?? "待处理";
  const recovery = progress?.mode === "recovery";
  const needsLogin = progress?.status === "auth_required";
  const startLabel = needsLogin
    ? "重新登录"
    : progress?.complete
      ? "归档新说说"
      : progress?.status === "error"
        ? recovery ? "继续查找" : "重试归档"
        : progress?.canResume ||
            progress?.status === "paused" ||
            progress?.status === "limited"
          ? recovery ? "继续查找" : "继续归档"
          : "开始归档";

  return (
    <section className="mx-auto max-w-5xl space-y-6">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <div className="flex items-center gap-2.5">
            <Archive size={23} className="text-primary" />
            <h1 className="text-xl font-semibold tracking-tight sm:text-2xl">
              说说归档
            </h1>
          </div>
          <p className="mt-2 text-sm text-muted-foreground">
            保存动态与互动，让每一次回望都有迹可循。
          </p>
        </div>
        <span className="inline-flex items-center gap-1.5 rounded-full border border-border px-3 py-1.5 text-xs text-muted-foreground">
          <ShieldCheck size={14} />
          数据保存在本机
        </span>
      </header>

      <div className="rounded-2xl border border-border bg-surface">
        <div className="flex flex-wrap items-center justify-between gap-4 px-5 py-5 sm:px-6">
          <div className="flex items-center gap-3">
            <span
              className={`grid size-10 place-items-center rounded-xl ${active ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground"}`}
            >
              {active ? (
                <LoaderCircle size={20} className="motion-safe:animate-spin" />
              ) : progress?.complete ? (
                <Check size={20} />
              ) : (
                <Database size={20} />
              )}
            </span>
            <h2 className="font-semibold">{recovery ? "历史说说查找" : "说说归档"}</h2>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {(active || progress?.canResume) && (
              <Button
                variant="ghost"
                disabled={busy || stopping}
                onClick={() => setConfirmAbandon(true)}
              >
                <X size={15} />
                放弃任务
              </Button>
            )}
            {active ? (
              <Button
                variant="outline"
                disabled={busy || stopping}
                onClick={() => void archive.action("cancel_feed_archive")}
              >
                <Pause size={15} />
                {stopping ? status : "暂停归档"}
              </Button>
            ) : (
              <Button
                disabled={busy || remaining > 0}
                onClick={() =>
                  void archive.action("start_feed_archive", {
                    intervalMs: 2000,
                    mode: recovery && !progress?.complete ? "recovery" : "own",
                  })
                }
              >
                {busy ? (
                  <LoaderCircle
                    size={15}
                    className="motion-safe:animate-spin"
                  />
                ) : (
                  <Play size={15} />
                )}
                {remaining > 0
                  ? `等待 ${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`
                  : startLabel}
              </Button>
            )}
          </div>
        </div>

        {progress?.message && (
          <div
            className="flex items-start gap-2 border-t border-border bg-muted/30 px-5 py-3 text-xs leading-5 text-muted-foreground"
            role="status"
          >
            <Clock3 size={14} className="mt-0.5 shrink-0" />
            <p className="break-words">{progress.message}</p>
          </div>
        )}
      </div>

      {confirmAbandon && (
        <div
          className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-500/30 bg-amber-500/5 p-4"
          role="alert"
        >
          <p className="text-sm">
            放弃将清除本次续传位置，保留已归档内容。下次会重新扫描。
          </p>
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => setConfirmAbandon(false)}>
              继续保留
            </Button>
            <Button
              variant="destructive"
              disabled={busy}
              onClick={() => {
                setConfirmAbandon(false);
                void archive.action("abandon_feed_archive");
              }}
            >
              确认放弃
            </Button>
          </div>
        </div>
      )}
      {archive.error && (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-xl border border-red-500/25 bg-red-500/5 p-4 text-sm text-red-500"
        >
          <CircleAlert size={17} className="mt-0.5 shrink-0" />
          <p className="break-words">{archive.error}</p>
        </div>
      )}
      {recovery && Boolean(progress?.skipped) && (
        <ArchiveRetries
          requireLogin={requireLogin}
          disabled={active || busy || remaining > 0}
          onRecovered={archive.recovered}
        />
      )}

      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="flex items-center gap-2 font-semibold">
          已归档内容
          <span className="rounded-full bg-muted px-2 py-0.5 text-xs font-normal text-muted-foreground">
            {progress?.archived?.toLocaleString() ?? 0}
          </span>
        </h2>
        <div className="flex items-center gap-2">
          <label className="flex h-9 items-center gap-2 rounded-lg border border-border bg-surface px-3">
            <Search size={15} className="text-muted-foreground" />
            <input
              aria-label="搜索归档内容"
              placeholder="搜索内容、昵称或 QQ 号"
              value={archive.query}
              onChange={(event) => archive.setQuery(event.target.value)}
              className="w-40 bg-transparent text-xs outline-none focus-visible:ring-0 sm:w-52"
            />
          </label>
          <Button
            aria-label="刷新归档内容"
            title="刷新归档内容"
            variant="outline"
            disabled={loading}
            className="px-2.5"
            onClick={() => void archive.recovered()}
          >
            <RefreshCw size={15} />
          </Button>
        </div>
      </div>
      <div className="space-y-4" aria-busy={loading}>
        {items.map((item) => (
          <ArchiveCard key={item.id} item={item} />
        ))}
      </div>
      {loading && (
        <p
          className="flex items-center justify-center gap-2 py-4 text-sm text-muted-foreground"
          role="status"
        >
          <LoaderCircle size={16} className="motion-safe:animate-spin" />
          正在读取归档…
        </p>
      )}
      {archive.more && (
        <Button
          variant="outline"
          className="mx-auto flex"
          disabled={loading}
          onClick={() => void archive.load(true)}
        >
          <ArrowDown size={15} />
          加载更多
        </Button>
      )}
      {!loading && !items.length && (
        <div className="grid place-items-center rounded-2xl border border-dashed border-border bg-surface px-5 py-14 text-center">
          <span className="grid size-12 place-items-center rounded-2xl bg-muted text-muted-foreground">
            <Archive size={23} />
          </span>
          <h3 className="mt-4 font-medium">
            {archive.query ? "没有匹配的归档内容" : "你的记忆，等待被保存"}
          </h3>
          <p className="mt-2 max-w-sm text-sm leading-6 text-muted-foreground">
            {archive.query
              ? "试试其他内容或昵称关键词。"
              : loggedIn
                ? "开始归档后，已保存的说说会显示在这里。"
                : "先登录 QQ 空间，再开始保存你的说说与互动记录。"}
          </p>
          {!loggedIn && (
            <Button className="mt-5" onClick={() => void requireLogin()}>
              登录 QQ 空间
            </Button>
          )}
        </div>
      )}
    </section>
  );
}
