import { createContext, useContext, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { Menu, X } from "lucide-react";
import { cn } from "../lib/utils";

type SidebarContextValue = {
  open: boolean;
  setOpen: Dispatch<SetStateAction<boolean>>;
  animate: boolean;
};

const SidebarContext = createContext<SidebarContextValue | null>(null);

function useSidebar() {
  const context = useContext(SidebarContext);
  if (!context) throw new Error("Sidebar components must be used within Sidebar.");
  return context;
}

export function Sidebar({
  children,
  open: openProp,
  setOpen: setOpenProp,
  animate = true,
}: {
  children: ReactNode;
  open?: boolean;
  setOpen?: Dispatch<SetStateAction<boolean>>;
  animate?: boolean;
}) {
  const [openState, setOpenState] = useState(false);
  const open = openProp ?? openState;
  const setOpen = setOpenProp ?? setOpenState;

  return <SidebarContext.Provider value={{ open, setOpen, animate }}>{children}</SidebarContext.Provider>;
}

export function DesktopSidebar({ children, className }: { children: ReactNode; className?: string }) {
  const { open, setOpen, animate } = useSidebar();
  const prefersReducedMotion = useReducedMotion();

  return (
    <motion.aside
      className={cn("hidden h-full shrink-0 flex-col border-r border-border bg-surface p-3 md:flex", className)}
      animate={{ width: animate && !prefersReducedMotion ? (open ? 208 : 84) : 208 }}
      transition={{ duration: 0.2, ease: "easeInOut" }}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
    >
      {children}
    </motion.aside>
  );
}

export function MobileSidebar({ children }: { children: ReactNode }) {
  const { open, setOpen } = useSidebar();
  const prefersReducedMotion = useReducedMotion();

  return (
    <>
      <button
        type="button"
        aria-label="打开导航菜单"
        aria-expanded={open}
        className="grid size-9 place-items-center rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground md:hidden"
        onClick={() => setOpen(true)}
      >
        <Menu size={20} />
      </button>
      <AnimatePresence>
        {open && (
          <motion.div className="fixed inset-0 z-50 md:hidden" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
            <button type="button" aria-label="关闭导航菜单" className="absolute inset-0 bg-black/40" onClick={() => setOpen(false)} />
            <motion.aside
              className="relative flex h-full w-[min(16rem,calc(100vw-3rem))] flex-col bg-surface p-4 shadow-xl"
              initial={{ x: "-100%" }}
              animate={{ x: 0 }}
              exit={{ x: "-100%" }}
              transition={{ duration: prefersReducedMotion ? 0 : 0.2, ease: "easeInOut" }}
            >
              <button type="button" aria-label="关闭导航菜单" className="absolute right-4 top-4 grid size-9 place-items-center rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground" onClick={() => setOpen(false)}>
                <X size={20} />
              </button>
              {children}
            </motion.aside>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}

export function SidebarLabel({ children }: { children: ReactNode }) {
  const { open } = useSidebar();
  return <motion.span animate={{ opacity: open ? 1 : 0 }} className={cn("whitespace-nowrap text-xs leading-none", !open && "pointer-events-none")}>{children}</motion.span>;
}
