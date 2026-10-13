import type { HTMLAttributes } from "react";
import { cn } from "../../lib/utils";

// Adapted from 21st's shadcnui-blocks/card-06 to the project's Vite stack and tokens.
export function Card({ className, ...props }: HTMLAttributes<HTMLElement>) {
  return <article className={cn("overflow-hidden rounded-2xl border border-border bg-surface text-foreground", className)} {...props} />;
}

export function CardHeader({ className, ...props }: HTMLAttributes<HTMLElement>) {
  return <header className={cn("flex items-center gap-3 px-5 py-4", className)} {...props} />;
}

export function CardContent({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("px-5 pb-5", className)} {...props} />;
}

export function CardFooter({ className, ...props }: HTMLAttributes<HTMLElement>) {
  return <footer className={cn("flex items-center gap-2 border-t border-border px-4 py-2", className)} {...props} />;
}
