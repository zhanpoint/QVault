import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { X } from "lucide-react";
import {
  ArchiveBox,
  Home2,
  Images,
  Login2,
  Moon,
  Sun,
  Users,
  MessageCircle,
  Star,
} from "reicon-react";
import { Button } from "./components/ui/button";
import {
  DesktopSidebar,
  MobileSidebar,
  Sidebar,
  SidebarLabel,
} from "./components/sidebar";
import { ArchivePage } from "./features/archive/ArchivePage";
import { ContactsPage } from "./features/contacts/ContactsPage";
import { AlbumsPage } from "./features/albums/AlbumsPage";
import { LibraryPage } from "./features/library/LibraryPage";
import "./styles/main.css";

type Route = { path: string; label: string; icon: typeof Home2 };

const routes: Route[] = [
  { path: "/", label: "概览", icon: Home2 },
  { path: "/archives", label: "归档", icon: ArchiveBox },
  { path: "/contacts", label: "联系人", icon: Users },
  { path: "/albums", label: "相册", icon: Images },
  { path: "/guestbook", label: "留言板", icon: MessageCircle },
  { path: "/favorites", label: "收藏", icon: Star },
];

type LoginStatus = { status: string; message: string; auth?: { uin: string } };
type QrLoginStart = { qrImage: string };

const delay = (milliseconds: number) =>
  new Promise((resolve) => setTimeout(resolve, milliseconds));

function currentPath() {
  return window.location.hash.slice(1) || "/";
}

function navigate(path: string) {
  window.location.hash = path;
}

function syncNativeTheme(theme: "dark" | "light") {
  if (!isTauri()) return;
  void getCurrentWindow()
    .setTheme(theme)
    .catch((error) => {
      console.error("Failed to sync the native window theme", error);
    });
}

function Placeholder() {
  return (
    <section className="mx-auto max-w-5xl space-y-5">
      <div className="rounded-2xl border border-border bg-surface p-6 shadow-sm">
        <div className="flex items-center gap-4">
          <span className="grid size-12 place-items-center rounded-xl bg-primary/10 text-primary">
            <Home2 size={23} />
          </span>
          <div>
            <p className="m-0 text-xs font-bold tracking-[.14em] text-primary">
              QVault
            </p>
            <h2 className="m-0 mt-1 text-xl font-bold">概览</h2>
            <p className="m-0 mt-1 text-sm text-muted-foreground">
              本地 QQ 空间数据归档中心
            </p>
          </div>
        </div>
      </div>
      <div className="grid min-h-72 place-items-center rounded-2xl border border-border bg-surface p-8 text-center shadow-sm">
        <div>
          <span className="mx-auto grid size-14 place-items-center rounded-2xl bg-muted text-primary">
            <ArchiveBox size={26} />
          </span>
          <h3 className="mt-4 text-lg font-bold">准备开始归档</h3>
          <p className="mt-2 max-w-md text-sm leading-6 text-muted-foreground">
            登录 QQ
            空间后，这里会显示你的本地归档数据。数据默认保存在当前设备上。
          </p>
          <Button
            className="mt-4"
            onClick={() => window.dispatchEvent(new Event("open-login"))}
          >
            <Login2 size={16} />
            登录 QQ 空间
          </Button>
        </div>
      </div>
    </section>
  );
}

function Dashboard() {
  return (
    <section className="mx-auto max-w-6xl space-y-5">
      <div className="relative overflow-hidden rounded-3xl border border-border bg-surface px-6 py-6 shadow-sm sm:px-9 sm:py-7 lg:px-11">
        <div
          aria-hidden="true"
          className="pointer-events-none absolute -right-16 -top-20 size-64 rounded-full bg-primary/10 blur-3xl"
        />
        <div className="relative space-y-6">
          <div className="text-center">
            <p className="m-0 flex items-center justify-center gap-3 text-xs font-semibold tracking-[.18em] text-primary">
              <span className="h-px w-8 bg-primary" />
              一切的起点
            </p>
            <h2 className="mt-4 text-2xl font-bold leading-tight tracking-tight sm:text-3xl lg:text-4xl">
              把散落在时间里的空间记忆，重新放回手边。
            </h2>
          </div>
          <div className="w-full border-t border-border pt-5 text-center">
            <p className="m-0 text-sm leading-6 text-foreground/85">
              一切开始于一个平常的午后。整理旧照片时，我们忽然想起那些曾经认真写下、后来又随手删掉的动态。那时总觉得日子还长，眼前的心情不值一提，等到多年以后再回头，才发现那已经是再也无法复刻的一天。
            </p>
            <p className="mt-3 text-sm leading-6 text-muted-foreground">
              我们删掉过青涩的文字，也删掉过一张并不完美的照片。可真正消失的，从来不只是几行内容：还有朋友当年的一句玩笑、深夜里收到的安慰，以及那个愿意坦率表达、尚未学会隐藏情绪的自己。
            </p>
            <p className="mt-3 text-sm leading-6 text-muted-foreground">
              后来才明白，记忆的价值并不取决于它是否体面。那些看似普通甚至有些笨拙的记录，恰恰标记着我们如何长大、如何爱过，又如何与一些重要的人渐渐走散。时间会带走很多东西，而被认真保存下来的片段，会替我们记住相遇真实发生过。
            </p>
            <p className="mt-3 text-sm leading-6 text-muted-foreground">
              QVault
              因此诞生。它不替回忆下定义，只希望把属于你的动态、照片与互动安静地留在手边。或许某一天再次翻开时，你会看见的不只是过去，也是在漫长岁月里一步步成为今天的自己。
            </p>
            <p className="mt-5 border-t border-border pt-4 text-sm font-medium text-foreground">
              归档不是告别，而是为重要的记忆留下一条回去的路。
            </p>
          </div>
        </div>
      </div>
      <section className="rounded-2xl border border-border bg-surface p-6 shadow-sm">
        <p className="m-0 text-center text-sm font-semibold leading-6 text-primary">
          所有数据都留存于本地，不会上传到外部；请仅归档本人或已获授权的内容。从未被点赞或评论过的动态无法恢复。
        </p>
      </section>
    </section>
  );
}

function SidebarNavigation({
  path,
  dark,
  onToggleTheme,
  onOpenLogin,
  setLoginUser,
  loginUser,
  onNavigate,
}: {
  path: string;
  dark: boolean;
  onToggleTheme: () => void;
  onOpenLogin: () => void;
  setLoginUser: (user: { uin: string } | null) => void;
  loginUser: { uin: string } | null;
  onNavigate?: () => void;
}) {
  const goTo = (nextPath: string) => {
    navigate(nextPath);
    onNavigate?.();
  };
  const startLogin = () => {
    onOpenLogin();
    onNavigate?.();
  };

  return (
    <>
      <div className="flex items-center gap-3 px-3 py-2">
        <img src="/QVault.svg" alt="QVault" className="size-10 shrink-0" />
        <SidebarLabel>
          <strong className="block">QVault</strong>
        </SidebarLabel>
      </div>
      <nav aria-label="主导航" className="mt-2 flex-1 space-y-1">
        {routes.map(({ path: itemPath, label, icon: Icon }) => (
          <button
            key={itemPath}
            type="button"
            onClick={() => goTo(itemPath)}
            title={label}
            className={`flex w-full items-center gap-3 rounded-xl px-3 py-3 text-left font-medium transition-colors ${path === itemPath ? "bg-primary/10 text-primary" : "text-muted-foreground hover:bg-muted hover:text-foreground"}`}
          >
            <Icon size={18} className="shrink-0" />
            <SidebarLabel>{label}</SidebarLabel>
          </button>
        ))}
      </nav>
      <div className="space-y-1 border-t border-border pt-2">
        <button
          type="button"
          onClick={onToggleTheme}
          title={dark ? "切换至浅色主题" : "切换至深色主题"}
          className="flex w-full items-center gap-3 rounded-xl px-3 py-3 text-left font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          {dark ? (
            <Sun size={18} className="shrink-0" />
          ) : (
            <Moon size={18} className="shrink-0" />
          )}
          <SidebarLabel>{dark ? "浅色主题" : "深色主题"}</SidebarLabel>
        </button>
        {loginUser ? (
          <button
            type="button"
            title={`已登录：${loginUser.uin}。悬浮后可退出登录`}
            onClick={() => {
              void invoke("logout_qzone")
                .then(() => setLoginUser(null))
                .catch(() => undefined);
            }}
            className="group flex w-full items-center gap-3 rounded-xl px-3 py-2 text-left text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <img
              src={`https://q1.qlogo.cn/g?b=qq&nk=${encodeURIComponent(loginUser.uin)}&s=100`}
              alt="QQ 头像"
              className="size-9 shrink-0 rounded-full object-cover"
            />
            <SidebarLabel>
              <span className="text-xs group-hover:hidden">
                {loginUser.uin}
              </span>
              <span className="hidden text-xs text-destructive group-hover:inline">
                退出登录
              </span>
            </SidebarLabel>
          </button>
        ) : (
          <button
            type="button"
            onClick={startLogin}
            title="登录 QQ 空间"
            className="flex w-full items-center gap-3 rounded-xl px-3 py-3 text-left font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <Login2
              size={18}
              color="var(--color-primary)"
              className="shrink-0"
            />
            <SidebarLabel>登录 QQ 空间</SidebarLabel>
          </button>
        )}
      </div>
    </>
  );
}

function AppShell() {
  const [path, setPath] = useState(currentPath());
  const [dark, setDark] = useState(true);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [loginOpen, setLoginOpen] = useState(false);
  const [qrImage, setQrImage] = useState("");
  const [loginMessage, setLoginMessage] = useState("正在获取登录二维码…");
  const [loginStatus, setLoginStatus] = useState("waiting");
  const [qrRefreshing, setQrRefreshing] = useState(false);
  const [loginUser, setLoginUser] = useState<{ uin: string } | null>(null);

  const requireLogin = async () => {
    try {
      const result = await invoke<LoginStatus>("get_login_status");
      if (result.status === "success" && result.auth) {
        setLoginUser((current) =>
          current?.uin === result.auth!.uin
            ? current
            : { uin: result.auth!.uin },
        );
        return true;
      }
    } catch (reason) {
      setLoginMessage(String(reason));
    }
    setLoginUser(null);
    setLoginOpen(true);
    return false;
  };

  useEffect(() => {
    const onHash = () => setPath(currentPath());
    const onLogin = () => setLoginOpen(true);
    window.addEventListener("hashchange", onHash);
    window.addEventListener("open-login", onLogin);
    syncNativeTheme("dark");
    void invoke<LoginStatus>("get_login_status")
      .then((result) => {
        if (result.status === "success" && result.auth)
          setLoginUser({ uin: result.auth.uin });
      })
      .catch(() => undefined);
    return () => {
      window.removeEventListener("hashchange", onHash);
      window.removeEventListener("open-login", onLogin);
    };
  }, []);

  useEffect(() => {
    if (!loginOpen) return;
    let cancelled = false;

    const startQrLogin = async () => {
      setQrImage("");
      setLoginStatus("waiting");
      setLoginMessage("正在获取登录二维码…");
      try {
        const started = await invoke<QrLoginStart>("start_qr_login");
        if (cancelled) return;
        setQrImage(started.qrImage);
        setLoginMessage("请使用手机 QQ 扫描二维码");
        while (!cancelled) {
          await delay(2000);
          if (cancelled) return;
          const result = await invoke<LoginStatus>("poll_qr_login");
          if (cancelled) return;
          setLoginStatus(result.status);
          setLoginMessage(result.message);
          if (result.status === "success") {
            if (result.auth) setLoginUser({ uin: result.auth.uin });
            setLoginMessage("登录成功");
            await delay(600);
            if (!cancelled) setLoginOpen(false);
            return;
          }
          if (result.status === "expired") {
            setLoginStatus("waiting");
            setQrRefreshing(true);
            await delay(300);
            if (cancelled) return;
            const refreshed = await invoke<QrLoginStart>("start_qr_login");
            if (cancelled) return;
            setQrImage(refreshed.qrImage);
            setQrRefreshing(false);
            setLoginMessage("请使用手机 QQ 扫描二维码");
            continue;
          }
          if (result.status === "error") return;
        }
      } catch (error) {
        if (cancelled) return;
        setQrRefreshing(false);
        setLoginStatus("error");
        setLoginMessage(
          String(error),
        );
      }
    };

    void startQrLogin();
    return () => {
      cancelled = true;
    };
  }, [loginOpen]);
  const toggleTheme = () => {
    setDark((current) => {
      const next = !current;
      syncNativeTheme(next ? "dark" : "light");
      return next;
    });
  };

  return (
    <div className={dark ? "dark h-full" : "h-full"}>
      <Sidebar open={sidebarOpen} setOpen={setSidebarOpen}>
        <div className="flex h-full bg-background text-foreground">
          <DesktopSidebar className="min-w-0 overflow-hidden">
            <SidebarNavigation
              path={path}
              dark={dark}
              loginUser={loginUser}
              setLoginUser={setLoginUser}
              onToggleTheme={toggleTheme}
              onOpenLogin={() => setLoginOpen(true)}
            />
          </DesktopSidebar>
          <div className="flex min-w-0 flex-1 flex-col">
            <header className="flex min-h-16 items-center border-b border-border bg-surface/80 px-4 backdrop-blur md:hidden">
              <MobileSidebar>
                <SidebarNavigation
                  path={path}
                  dark={dark}
                  loginUser={loginUser}
                  setLoginUser={setLoginUser}
                  onToggleTheme={toggleTheme}
                  onOpenLogin={() => setLoginOpen(true)}
                  onNavigate={() => setSidebarOpen(false)}
                />
              </MobileSidebar>
            </header>
            <main className="app-scroll flex-1 p-4 sm:p-7">
              {path === "/" ? (
                <Dashboard />
              ) : path === "/archives" ? (
                <ArchivePage
                  key={loginUser?.uin ?? "logged-out"}
                  loggedIn={Boolean(loginUser)}
                  requireLogin={requireLogin}
                />
              ) : path === "/contacts" ? (
                <ContactsPage
                  key={loginUser?.uin ?? "logged-out"}
                  ownerUin={loginUser?.uin ?? null}
                  requireLogin={requireLogin}
                />
              ) : path === "/albums" ? (
                <AlbumsPage
                  key={loginUser?.uin ?? "logged-out"}
                  loggedIn={Boolean(loginUser)}
                  requireLogin={requireLogin}
                />
              ) : path === "/guestbook" || path === "/favorites" ? (
                <LibraryPage
                  key={`${path}-${loginUser?.uin ?? "logged-out"}`}
                  module={path === "/guestbook" ? "guestbook" : "favorites"}
                  loggedIn={Boolean(loginUser)}
                  requireLogin={requireLogin}
                />
              ) : (
                <Placeholder />
              )}
            </main>
          </div>
        </div>
      </Sidebar>
      {loginOpen && (
        <div
          className="fixed inset-0 z-50 grid place-items-center bg-black/45 p-4"
          role="dialog"
          aria-modal="true"
          aria-labelledby="login-title"
        >
          <div className="w-full max-w-md rounded-3xl border border-border bg-surface p-6 shadow-2xl sm:p-8">
            <div className="flex items-start justify-between gap-4">
              <div>
                <p className="m-0 text-xs font-semibold tracking-[.16em] text-primary">
                  QVault
                </p>
                <h2 id="login-title" className="mt-2 text-2xl font-bold">
                  登录 QQ 空间
                </h2>
              </div>
              <button
                type="button"
                aria-label="关闭登录窗口"
                className="grid size-9 place-items-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground"
                onClick={() => setLoginOpen(false)}
              >
                <X size={20} />
              </button>
            </div>
            <div className="mt-7 flex flex-col items-center text-center">
              <div className="relative grid size-64 place-items-center overflow-hidden rounded-3xl border border-border bg-white p-4 shadow-sm sm:size-72">
                {qrImage ? (
                  <img
                    src={qrImage}
                    alt="QQ 登录二维码"
                    className={`size-full object-contain transition-opacity duration-150 ${qrRefreshing ? "opacity-20" : "opacity-100"}`}
                    style={{ imageRendering: "pixelated" }}
                  />
                ) : (
                  <div className="text-sm text-slate-500">正在获取二维码…</div>
                )}
                {qrRefreshing && (
                  <div className="absolute inset-0 grid place-items-center bg-white/55">
                    <span className="size-7 animate-spin rounded-full border-2 border-slate-300 border-t-blue-500" />
                  </div>
                )}
              </div>
              <p
                className={`mt-5 text-base font-semibold ${loginStatus === "error" || loginStatus === "expired" ? "text-red-500" : "text-foreground"}`}
              >
                {loginMessage}
              </p>
              {loginStatus === "error" && (
                <button
                  type="button"
                  className="mt-4 text-sm font-semibold text-primary hover:underline"
                  onClick={() => {
                    setLoginOpen(false);
                    setTimeout(() => setLoginOpen(true), 0);
                  }}
                >
                  重新获取二维码
                </button>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default AppShell;
