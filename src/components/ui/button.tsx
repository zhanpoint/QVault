import type { ButtonHTMLAttributes } from "react";
import { cn } from "../../lib/utils";

type Variant = "default" | "outline" | "ghost" | "secondary" | "destructive";
export function Button({ className, variant = "default", ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  const styles: Record<Variant, string> = {
    default: "bg-primary text-white shadow-sm hover:bg-primary/90",
    outline: "border border-border bg-surface hover:bg-muted/40",
    ghost: "text-muted-foreground hover:bg-muted/50 hover:text-foreground",
    secondary: "bg-muted text-foreground hover:bg-muted/80",
    destructive: "bg-red-500 text-white hover:bg-red-600",
  };
  return <button className={cn("inline-flex h-9 items-center justify-center gap-2 rounded-lg px-3 text-sm font-semibold transition-[transform,colors,box-shadow] duration-150 hover:-translate-y-px hover:shadow-sm active:translate-y-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/50 focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:pointer-events-none disabled:opacity-50", styles[variant], className)} {...props} />;
}
