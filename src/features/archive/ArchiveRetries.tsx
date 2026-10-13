import { invoke } from "@tauri-apps/api/core";
import { useRef, useState } from "react";
import { CircleAlert, LoaderCircle, RefreshCw } from "lucide-react";
import { Button } from "../../components/ui/button";

type Skip = { id: number; pageNumber: number; error: string; offsetAdvance: number; retryCount: number; resolvedAt?: number | null };

export function ArchiveRetries({ disabled, onRecovered, requireLogin }: { disabled: boolean; onRecovered: () => Promise<void>; requireLogin: () => Promise<boolean> }) {
  const [items, setItems] = useState<Skip[]>([]);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const pending = useRef(false);
  const run = async (operation: () => Promise<void>) => {
    if (disabled || pending.current) return;
    pending.current = true; setBusy(true); setMessage("");
    try { if (await requireLogin()) await operation(); }
    catch (error) { setMessage(String(error)); }
    finally { pending.current = false; setBusy(false); }
  };
  const load = async () => setItems((await invoke<Skip[]>("list_archive_skips")).filter((item) => !item.resolvedAt));
  const retry = (id: number) => run(async () => {
    const result = await invoke<{ success: boolean; message: string }>("retry_archive_skip", { id });
    setMessage(result.message);
    await load();
    await onRecovered();
  });

  return <details className="rounded-2xl border border-amber-500/25 bg-surface p-5" onToggle={(event) => { if (event.currentTarget.open) void run(load); }}>
    <summary className="cursor-pointer text-sm font-medium"><span className="inline-flex items-center gap-2"><CircleAlert size={16} className="text-amber-500" />待补充的异常位置</span></summary>
    <p className="mt-3 text-xs leading-6 text-muted-foreground">这里保留之前互动补充失败的位置，可单独重试。历史说说恢复使用独立进度。</p>
    <Button className="mt-3" variant="outline" disabled={disabled || busy} onClick={() => void run(load)}>{busy ? <LoaderCircle size={14} className="motion-safe:animate-spin" /> : <RefreshCw size={14} />}刷新列表</Button>
    {message && <p role="status" className="mt-3 break-words text-xs leading-6 text-muted-foreground">{message}</p>}
    <div className="mt-3 divide-y divide-border">{items.map((item) => <div key={item.id} className="flex flex-wrap items-start justify-between gap-3 py-4"><div className="min-w-0 flex-1"><p className="text-sm font-medium">第 {item.pageNumber} 页<span className="ml-2 text-xs font-normal text-muted-foreground">{item.offsetAdvance > 0 ? "已标记跳过" : "尚未跳过"} · 重试 {item.retryCount} 次</span></p><p className="mt-1 break-words text-xs leading-6 text-muted-foreground">{item.error}</p></div><div className="flex gap-2"><Button variant="outline" disabled={disabled || busy} onClick={() => void retry(item.id)}><RefreshCw size={13} />重试</Button></div></div>)}</div>
  </details>;
}
