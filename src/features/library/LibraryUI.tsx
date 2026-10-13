import {
  ChevronLeft,
  ChevronRight,
  Images,
  type LucideIcon,
} from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "../../components/ui/button";

export function EmptyState({
  title,
  description,
  children,
  icon: Icon = Images,
}: {
  title: string;
  description: string;
  children?: ReactNode;
  icon?: LucideIcon;
}) {
  return (
    <div className="grid min-h-64 place-content-center rounded-2xl border border-dashed border-border bg-surface px-6 py-10 text-center">
      <Icon size={30} className="mx-auto mb-4 text-muted-foreground" />
      <h3 className="m-0 text-base font-semibold">{title}</h3>
      <p className="mx-auto mt-2 max-w-md text-sm leading-6 text-muted-foreground">
        {description}
      </p>
      {children && (
        <div className="mt-4 flex justify-center gap-2">{children}</div>
      )}
    </div>
  );
}

export function ErrorNotice({ error }: { error: string }) {
  return error ? (
    <p
      role="alert"
      className="rounded-xl border border-red-500/20 bg-red-500/5 px-4 py-3 text-sm leading-6 text-red-600 dark:text-red-400"
    >
      {error}
    </p>
  ) : null;
}

export function Pager({
  page,
  loading,
  more,
  onPrevious,
  onNext,
}: {
  page: number;
  loading: boolean;
  more: boolean;
  onPrevious: () => void;
  onNext: () => void;
}) {
  return (
    <nav
      aria-label="资源分页"
      className="flex items-center justify-center gap-4 border-t border-border pt-5"
    >
      <Button
        variant="outline"
        disabled={loading || page === 0}
        onClick={onPrevious}
      >
        <ChevronLeft size={16} />
        上一页
      </Button>
      <span
        aria-live="polite"
        className="text-xs tabular-nums text-muted-foreground"
      >
        第 {page + 1} 页
      </span>
      <Button variant="outline" disabled={loading || !more} onClick={onNext}>
        下一页
        <ChevronRight size={16} />
      </Button>
    </nav>
  );
}
