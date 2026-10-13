import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type { LibraryModule, LibraryPage } from "./types";

export const PAGE_SIZE = 24;
export const errorMessage = (reason: unknown) =>
  reason instanceof Error ? reason.message : String(reason);

export function useResources(
  module: LibraryModule,
  parentKey: string | null,
  loggedIn: boolean,
  requireLogin: () => Promise<boolean>,
  filters: {
    query?: string;
    year?: number | null;
    source?: "current" | "history" | null;
  } = {},
) {
  const { query, year, source } = filters;
  const [data, setData] = useState<LibraryPage | null>(null);
  const [page, setPage] = useState(0);
  const [loading, setLoading] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const mounted = useRef(false);
  const request = useRef(0);
  const taskId = useRef<string | null>(null);
  const pending = useRef(false);
  const load = useCallback(
    async (next = 0) => {
      if (!loggedIn) return;
      const run = ++request.current;
      setLoading(true);
      setError("");
      try {
        const result = await invoke<LibraryPage>("list_qzone_library", {
          request: {
            module,
            parentKey,
            query: query || null,
            year: year ?? null,
            source: source ?? null,
            limit: PAGE_SIZE,
            offset: next * PAGE_SIZE,
          },
        });
        if (!mounted.current || request.current !== run) return;
        setData(result);
        setPage(next);
        setError(result.lastError ?? "");
      } catch (reason) {
        if (mounted.current && request.current === run)
          setError(errorMessage(reason));
      } finally {
        if (mounted.current && request.current === run) setLoading(false);
      }
    },
    [loggedIn, module, parentKey, query, year, source],
  );

  useEffect(() => {
    mounted.current = true;
    setData(null);
    setPage(0);
    void load();
    return () => {
      mounted.current = false;
      request.current++;
      if (taskId.current)
        void invoke("cancel_feed_archive", { taskId: taskId.current }).catch(
          () => undefined,
        );
    };
  }, [load]);

  useEffect(() => {
    if (!syncing) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const progress = await invoke<{
          active: boolean;
          message: string;
          taskId?: string;
        }>("get_archive_progress");
        if (
          !cancelled &&
          progress.active &&
          progress.taskId === taskId.current
        ) {
          setMessage(progress.message);
        }
      } catch {
        /* Sync's own response reports errors. */
      }
      if (!cancelled) timer = setTimeout(() => void poll(), 1200);
    };
    void poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [syncing]);

  const sync = async () => {
    if (pending.current) return;
    pending.current = true;
    try {
      if (!(await requireLogin()) || !mounted.current) return;
      setSyncing(true);
      taskId.current = crypto.randomUUID();
      setError("");
      setMessage("正在同步 QQ 空间…");
      const result = await invoke<{ message: string }>("sync_qzone_library", {
        module,
        parentKey,
        taskId: taskId.current,
      });
      if (mounted.current) {
        setMessage(result.message);
        await load(0);
      }
    } catch (reason) {
      if (mounted.current) {
        await load(0);
        if (!mounted.current) return;
        if (errorMessage(reason) === "ARCHIVE_PAUSED") {
          setMessage("已暂停，已保存的记录和同步断点会保留。");
          setError("");
        } else {
          setMessage("");
          setError(errorMessage(reason));
        }
      }
    } finally {
      taskId.current = null;
      pending.current = false;
      if (mounted.current) setSyncing(false);
    }
  };
  const pause = async () => {
    if (!taskId.current) return;
    try {
      await invoke("cancel_feed_archive", { taskId: taskId.current });
    } catch (reason) {
      if (mounted.current) setError(errorMessage(reason));
    }
  };
  return { data, page, loading, syncing, message, error, load, sync, pause };
}
