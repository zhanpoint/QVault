import { invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ArchiveItem, ArchiveProgress } from "./types";

const PAGE_SIZE = 20;
const isActive = (progress: ArchiveProgress | null) =>
  Boolean(progress?.active);

function archiveRevision(progress: ArchiveProgress | null) {
  return progress
    ? `${progress.mode}:${progress.saved}:${progress.archived}`
    : "";
}

function mergeArchiveItems(current: ArchiveItem[], incoming: ArchiveItem[]) {
  const byId = new Map(current.map((item) => [item.id, item]));
  for (const item of incoming) byId.set(item.id, item);
  return [...byId.values()].sort(
    (a, b) => b.publishedAt - a.publishedAt || b.id - a.id,
  );
}

export function useArchive(
  loggedIn: boolean,
  requireLogin: () => Promise<boolean>,
) {
  const [progress, setProgress] = useState<ArchiveProgress | null>(null);
  const [items, setItems] = useState<ArchiveItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [more, setMore] = useState(false);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [search, setSearch] = useState("");
  const [clock, setClock] = useState(Date.now());
  const mounted = useRef(false);
  const sequence = useRef(0);
  const last = useRef<ArchiveItem | undefined>(undefined);
  const currentProgress = useRef<ArchiveProgress | null>(null);
  const loadLatest = useRef<(live?: boolean) => Promise<void>>(async () => {});
  const visibleItems = useRef<ArchiveItem[]>([]);
  const loadedRevision = useRef("");
  const progressRequest = useRef<{
    account: number;
    promise: Promise<void>;
  } | null>(null);
  const listRequest = useRef(false);
  const actionRequest = useRef(false);
  const authVersion = useRef(0);
  const previousLogin = useRef(loggedIn);
  const available = isTauri();

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      sequence.current++;
    };
  }, []);

  useEffect(() => {
    if (previousLogin.current !== loggedIn) {
      previousLogin.current = loggedIn;
      authVersion.current++;
    }
    if (!loggedIn) {
      sequence.current++;
      last.current = undefined;
      currentProgress.current = null;
      listRequest.current = false;
      setProgress(null);
      setItems([]);
      visibleItems.current = [];
      loadedRevision.current = "";
      setMore(false);
      setLoading(false);
      setError("");
    }
  }, [loggedIn]);

  useEffect(() => {
    const timer = window.setTimeout(() => setSearch(query.trim()), 250);
    return () => window.clearTimeout(timer);
  }, [query]);

  const load = useCallback(
    async (append = false, live = false) => {
      if (!loggedIn || !available || ((append || live) && listRequest.current))
        return;
      const revision = archiveRevision(currentProgress.current);
      const request = ++sequence.current;
      listRequest.current = true;
      let committed = false;
      if (!live) setLoading(true);
      const boundary = append ? last.current : undefined;
      try {
        const fetchPage = (cursor: ArchiveItem | undefined, limit: number) =>
          invoke<ArchiveItem[]>("list_archived_feeds", {
            request: {
              category: "self",
              limit,
              offset: 0,
              query: search || null,
              beforeTime: cursor?.publishedAt ?? null,
              beforeId: cursor?.id ?? null,
            },
          });
        const result = await fetchPage(boundary, PAGE_SIZE + 1);
        if (!mounted.current || request !== sequence.current) return;
        const page = result.slice(0, PAGE_SIZE);
        const current = visibleItems.current;
        const nextItems = live || append ? mergeArchiveItems(current, page) : page;
        const tail = nextItems.at(-1);
        const hasMore = live
          ? search
            ? Boolean(tail && (await fetchPage(tail, 1)).length > 0)
            : (currentProgress.current?.archived ?? nextItems.length) >
              nextItems.length
          : result.length > PAGE_SIZE;
        if (!mounted.current || request !== sequence.current) return;
        visibleItems.current = nextItems;
        setItems(nextItems);
        last.current = tail;
        setMore(hasMore);
        loadedRevision.current = revision;
        committed = true;
      } catch (reason) {
        if (mounted.current && request === sequence.current)
          setError(String(reason));
      } finally {
        if (mounted.current && request === sequence.current) {
          setLoading(false);
          listRequest.current = false;
          if (
            committed &&
            currentProgress.current?.stage !== "library" &&
            archiveRevision(currentProgress.current) !== loadedRevision.current
          )
            void loadLatest.current(true);
        }
      }
    },
    [loggedIn, available, search],
  );
  loadLatest.current = (live = false) => load(false, live);

  const refresh = useCallback(() => {
    if (!loggedIn || !available) return Promise.resolve();
    const account = authVersion.current;
    if (progressRequest.current?.account === account)
      return progressRequest.current.promise;
    const promise = (async () => {
      try {
        const next = await invoke<ArchiveProgress>("get_archive_progress");
        if (!mounted.current || account !== authVersion.current) return;
        const finished = isActive(currentProgress.current) && !isActive(next);
        currentProgress.current = next;
        setProgress(next);
        if (
          next.stage !== "library" &&
          (finished || archiveRevision(next) !== loadedRevision.current)
        )
          await loadLatest.current(true);
      } catch (reason) {
        if (mounted.current && account === authVersion.current)
          setError(String(reason));
      } finally {
        if (progressRequest.current?.account === account)
          progressRequest.current = null;
      }
    })();
    progressRequest.current = { account, promise };
    return promise;
  }, [loggedIn, available]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => {
      setClock(Date.now());
      if (
        document.visibilityState === "visible" &&
        currentProgress.current?.active
      )
        void refresh();
    }, 2000);
    const visible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", visible);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [refresh]);

  const action = async (command: string, args?: Record<string, unknown>) => {
    if (actionRequest.current) return;
    if (!available) {
      setError("完整归档功能需要桌面运行环境；浏览器开发模式可预览页面。");
      return;
    }
    actionRequest.current = true;
    setBusy(true);
    setError("");
    try {
      if (!(await requireLogin())) return;
      const result = await invoke<ArchiveProgress | undefined>(command, args);
      if (!mounted.current) return;
      if (command === "start_feed_archive" && result) {
        currentProgress.current = result;
        setProgress(result);
      }
      await refresh();
    } catch (reason) {
      if (mounted.current) setError(String(reason));
    } finally {
      actionRequest.current = false;
      if (mounted.current) setBusy(false);
    }
  };

  return {
    progress,
    items,
    loading,
    busy,
    more,
    error,
    query,
    setQuery,
    available,
    remaining: Math.max(
      0,
      Math.ceil(((progress?.retryAt ?? 0) * 1000 - clock) / 1000),
    ),
    load,
    refresh,
    action,
    recovered: async () => {
      await Promise.all([refresh(), load()]);
    },
  };
}
