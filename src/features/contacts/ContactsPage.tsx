import {
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  ArrowDown,
  ArrowLeft,
  CalendarDays,
  ChevronRight,
  ChevronDown,
  Check,
  CircleAlert,
  LoaderCircle,
  MessageCircle,
  RefreshCw,
  Search,
  Users,
} from "lucide-react";
import * as Select from "@radix-ui/react-select";
import { QzoneText } from "../../components/QzoneText";
import { Button } from "../../components/ui/button";
import { ArchiveCard } from "../archive/ArchiveCard";
import type { Contact } from "./types";
import { useContacts } from "./useContacts";

const number = new Intl.NumberFormat("zh-CN");
const date = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "long",
  day: "numeric",
});
const formatDate = (time: number) => date.format(new Date(time * 1000));
type Sort = "posts" | "lastAt";

function Avatar({ contact }: { contact: Contact }) {
  return (
    <span
      aria-hidden="true"
      className="grid size-10 shrink-0 place-items-center rounded-full bg-primary/10 text-sm font-semibold text-primary"
    >
      {Array.from(contact.nickname)[0]}
    </span>
  );
}

function Empty({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="grid min-h-64 place-items-center rounded-2xl border border-dashed border-border bg-surface p-6 text-center">
      <div>
        <Users size={28} className="mx-auto text-muted-foreground" />
        <h2 className="mt-4 font-semibold">{title}</h2>
        <div className="mt-2 text-sm leading-6 text-muted-foreground">
          {children}
        </div>
      </div>
    </div>
  );
}

function ContactProfile({
  contact,
  back,
}: {
  contact: Contact;
  back: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const message = contact.lastMessage;
  const longMessage =
    message && (message.content.length > 120 || message.content.includes("\n"));
  const dates =
    contact.firstAt > 0
      ? formatDate(contact.firstAt) === formatDate(contact.lastAt)
        ? [{ label: "记录中的互动日期", time: contact.firstAt }]
        : [
            { label: "最早留下的互动", time: contact.firstAt },
            { label: "最近留下的互动", time: contact.lastAt },
          ]
      : [];

  return (
    <div className="rounded-2xl border border-border bg-surface p-4">
      <Button
        variant="ghost"
        className="mb-4 -ml-3 h-8 text-xs lg:hidden"
        onClick={back}
      >
        <ArrowLeft size={14} />
        返回联系人
      </Button>
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
      <div className="flex min-w-0 items-center gap-3">
        <Avatar contact={contact} />
        <div className="min-w-0">
          <h2 className="break-words text-lg font-semibold">
            {contact.nickname}
          </h2>
          <p className="mt-1 text-xs text-muted-foreground">QQ {contact.uin}</p>
        </div>
      </div>
      <dl className="flex flex-wrap gap-x-6 gap-y-2">
        <div>
          <dt className="text-xs text-muted-foreground">共同留下的说说</dt>
          <dd className="mt-1 text-lg font-semibold tabular-nums">
            {number.format(contact.posts)}
            <span className="ml-1.5 text-xs font-normal text-muted-foreground">
              条
            </span>
          </dd>
        </div>
        {contact.dialoguePosts > 0 && (
          <div>
            <dt className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <MessageCircle size={13} />
              其中有来有回
            </dt>
            <dd className="mt-1 text-lg font-semibold tabular-nums">
              {number.format(contact.dialoguePosts)}
              <span className="ml-1.5 text-xs font-normal text-muted-foreground">
                条
              </span>
            </dd>
          </div>
        )}
      </dl>
      </div>
      {message && (
        <blockquote className="mt-3 border-l-2 border-primary/30 pl-3">
          <p className="text-xs text-muted-foreground">
            TA 留下的话
            {message.createdAt > 0 && (
              <span className="ml-2">{formatDate(message.createdAt)}</span>
            )}
          </p>
          <p
            className={`mt-2 whitespace-pre-wrap break-words text-sm leading-7 ${longMessage && !expanded ? "line-clamp-3" : ""}`}
          >
            <QzoneText text={message.content} />
          </p>
          {longMessage && (
            <button
              type="button"
              aria-expanded={expanded}
              onClick={() => setExpanded((current) => !current)}
              className="mt-1 text-xs text-primary"
            >
              {expanded ? "收起留言" : "展开留言"}
            </button>
          )}
        </blockquote>
      )}
      {dates.length > 0 && (
        <dl className="mt-3 flex flex-wrap gap-x-8 gap-y-2 border-t border-border pt-3">
          {dates.map(({ label, time }) => (
            <div key={label} className="flex items-start gap-2">
              <CalendarDays
                size={14}
                className="mt-0.5 shrink-0 text-muted-foreground"
              />
              <div>
                <dt className="text-xs text-muted-foreground">{label}</dt>
                <dd className="mt-1 text-sm">
                  <time dateTime={new Date(time * 1000).toISOString()}>
                    {formatDate(time)}
                  </time>
                </dd>
              </div>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}

export function ContactsPage({
  ownerUin,
  requireLogin,
}: {
  ownerUin: string | null;
  requireLogin: () => Promise<boolean>;
}) {
  const data = useContacts(ownerUin);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("posts");
  const [visible, setVisible] = useState(40);
  const deferredQuery = useDeferredValue(query.trim().toLocaleLowerCase());
  const contacts = data.directory?.contacts;
  const filtered = useMemo(
    () =>
      (contacts ?? [])
        .filter(
          (contact) =>
            contact.uin.includes(deferredQuery) ||
            contact.nickname.toLocaleLowerCase().includes(deferredQuery),
        )
        .sort(
          (a, b) =>
            b[sort] - a[sort] ||
            b.posts - a.posts ||
            b.lastAt - a.lastAt ||
            a.uin.localeCompare(b.uin),
        ),
    [contacts, deferredQuery, sort],
  );
  const selected = contacts?.find(
    (contact) => contact.uin === data.selectedUin,
  );
  useEffect(() => {
    setVisible(40);
  }, [deferredQuery, sort, contacts]);

  let content: ReactNode = null;
  if (!ownerUin) {
    content = (
      <Empty title="登录后，回看归档中的相遇">
        <p>从保存的说说与评论中，回看一起留下的记录。</p>
        <Button className="mt-4" onClick={() => void requireLogin()}>
          登录 QQ 空间
        </Button>
      </Empty>
    );
  } else if (!data.available) {
    content = (
      <Empty title="浏览器可预览联系人页面">
        <p>读取本地归档需要桌面运行环境。</p>
      </Empty>
    );
  } else if (data.loading && !data.directory) {
    content = (
      <p
        role="status"
        className="flex items-center justify-center gap-2 py-16 text-sm text-muted-foreground"
      >
        <LoaderCircle size={18} className="motion-safe:animate-spin" />
        正在整理共同留下的记录…
      </p>
    );
  } else if (data.directory && !contacts?.length) {
    content = (
      <Empty title="回忆还没有归档到这里">
        <p>先归档说说及互动，曾经留下点赞和评论的人会出现在这里。</p>
        <Button
          variant="outline"
          className="mt-4"
          onClick={() => {
            window.location.hash = "/archives";
          }}
        >
          前往归档
        </Button>
      </Empty>
    );
  } else if (data.directory) {
    content = (
      <div className="grid min-h-0 gap-5 lg:flex-1 lg:grid-cols-[300px_minmax(0,1fr)]">
        {/* Compact person rows adapted from 21st's Item List with Actions. */}
        <aside
          aria-label="归档联系人列表"
          className={`${selected ? "hidden lg:flex" : "flex"} min-h-0 flex-col rounded-2xl border border-border bg-surface`}
        >
          <div className="space-y-3 border-b border-border p-4">
            <label className="flex items-center gap-2 rounded-lg border border-border bg-background px-3 py-2">
              <Search size={15} className="shrink-0 text-muted-foreground" />
              <input
                type="search"
                aria-label="搜索昵称或 QQ 号"
                placeholder="搜索昵称或 QQ 号"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                className="min-w-0 w-full rounded-sm bg-transparent text-sm outline-none focus-visible:ring-2 focus-visible:ring-primary/50"
              />
            </label>
            <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
              <span>{number.format(filtered.length)} 位联系人</span>
              <Select.Root value={sort} onValueChange={(value) => setSort(value as Sort)}>
                <Select.Trigger aria-label="联系人排序" className="inline-flex h-9 w-36 items-center justify-between gap-2 rounded-md border border-border bg-background px-3 text-xs text-foreground focus-visible:outline-2 focus-visible:outline-primary">
                  <Select.Value />
                  <Select.Icon><ChevronDown size={14} /></Select.Icon>
                </Select.Trigger>
                <Select.Portal>
                  <Select.Content position="popper" sideOffset={4} className="z-50 min-w-[var(--radix-select-trigger-width)] overflow-hidden rounded-md border border-border bg-surface p-1 text-foreground shadow-lg">
                    <Select.Viewport>
                      {([{ value: "posts", label: "共同说说最多" }, { value: "lastAt", label: "最近有互动" }] as const).map((option) => (
                        <Select.Item key={option.value} value={option.value} className="relative flex cursor-default select-none items-center rounded px-3 py-2 pr-8 text-xs outline-none data-[highlighted]:bg-muted">
                          <Select.ItemText>{option.label}</Select.ItemText>
                          <Select.ItemIndicator className="absolute right-2 text-primary"><Check size={14} /></Select.ItemIndicator>
                        </Select.Item>
                      ))}
                    </Select.Viewport>
                  </Select.Content>
                </Select.Portal>
              </Select.Root>
            </div>
          </div>
          <div
            className="app-scroll max-h-[65vh] min-h-0 p-2 lg:max-h-none lg:flex-1"
            aria-busy={data.loading}
          >
            {filtered.slice(0, visible).map((contact) => (
              <button
                type="button"
                key={contact.uin}
                onClick={() => data.select(contact.uin)}
                aria-pressed={selected?.uin === contact.uin}
                aria-controls="contact-detail"
                className={`flex w-full items-center gap-3 rounded-xl px-3 py-3.5 text-left ${selected?.uin === contact.uin ? "bg-primary/10" : "hover:bg-muted/60"}`}
              >
                <Avatar contact={contact} />
                <span className="min-w-0 flex-1">
                  <strong
                    title={contact.nickname}
                    className="block truncate text-sm font-semibold"
                  >
                    {contact.nickname}
                  </strong>
                  <span className="mt-1 block text-xs text-muted-foreground">
                    {number.format(contact.posts)} 条共同说说
                  </span>
                </span>
                <ChevronRight
                  size={14}
                  className="shrink-0 text-muted-foreground"
                />
              </button>
            ))}
            {!filtered.length && (
              <p className="p-6 text-center text-sm text-muted-foreground">
                没有找到匹配的联系人
              </p>
            )}
            {filtered.length > visible && (
              <Button
                variant="ghost"
                className="mt-2 w-full text-xs"
                onClick={() => setVisible((current) => current + 40)}
              >
                显示更多联系人
              </Button>
            )}
          </div>
        </aside>
        <div
          id="contact-detail"
          className={`min-h-0 min-w-0 lg:flex lg:flex-col lg:gap-5 ${selected ? "" : "hidden"}`}
        >
          {selected ? (
            <>
              <div className="mb-5 lg:mb-0 lg:max-h-[45%] lg:shrink-0 lg:overflow-y-auto">
              <ContactProfile
                key={selected.uin}
                contact={selected}
                back={() => data.select(null)}
              />
              </div>
              <div className="mb-5 lg:mb-0 lg:shrink-0">
                <h3 className="text-sm font-semibold">一起留下的回忆</h3>
                <p className="mt-1 text-xs text-muted-foreground">
                  回到当时的说说，看看原来的留言与回复。
                </p>
              </div>
              <div key={selected.uin} className="app-scroll min-h-0 space-y-4 lg:flex-1 lg:pr-2">
              {data.feeds.map((item) => (
                <ArchiveCard key={item.id} item={item} />
              ))}
              {data.feedsLoading && (
                <p
                  role="status"
                  className="flex items-center justify-center gap-2 py-5 text-sm text-muted-foreground"
                >
                  <LoaderCircle
                    size={16}
                    className="motion-safe:animate-spin"
                  />
                  正在读取说说…
                </p>
              )}
              {data.feedsError && (
                <div
                  role="alert"
                  className="rounded-xl border border-red-500/25 p-4 text-sm"
                >
                  <p className="break-words text-red-500">{data.feedsError}</p>
                  <Button
                    variant="outline"
                    className="mt-3"
                    onClick={() => void data.retryFeeds()}
                  >
                    重试读取
                  </Button>
                </div>
              )}
              {!data.feedsLoading && !data.feedsError && !data.feeds.length && (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  关联说说已发生变化，请刷新。
                </p>
              )}
              {data.more && !data.feedsError && (
                <Button
                  variant="outline"
                  disabled={data.feedsLoading}
                  className="mx-auto flex"
                  onClick={() => void data.loadMore()}
                >
                  <ArrowDown size={14} />
                  加载更多说说
                </Button>
              )}
              </div>
            </>
          ) : (
            <Empty title="选一个人，回看当年的记录">
              <p>说说里的点赞、留言与回复，都保留在原来的上下文中。</p>
            </Empty>
          )}
        </div>
      </div>
    );
  }

  return (
    <section className="mx-auto max-w-6xl space-y-5 lg:flex lg:h-full lg:min-h-0 lg:flex-col lg:space-y-0 lg:gap-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="flex items-center gap-2.5 text-xl font-semibold tracking-tight">
            <Users size={23} className="text-primary" />
            联系人
          </h1>
          <p className="mt-2 text-sm text-muted-foreground">
            找回一起留下的说说与对话。
          </p>
        </div>
        <Button
          variant="outline"
          disabled={!ownerUin || !data.available || data.loading}
          onClick={data.refresh}
        >
          <RefreshCw
            size={15}
            className={data.loading ? "motion-safe:animate-spin" : ""}
          />
          刷新
        </Button>
      </header>
      {data.error && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-2 rounded-xl border border-red-500/25 bg-red-500/5 p-4 text-sm text-red-500"
        >
          <CircleAlert size={16} />
          <p className="min-w-0 flex-1 break-words">{data.error}</p>
          <Button
            variant="outline"
            onClick={data.refresh}
            disabled={data.loading}
          >
            重试
          </Button>
        </div>
      )}
      {content}
      {data.directory && (
        <details className="shrink-0 text-xs leading-6 text-muted-foreground">
          <summary className="w-fit cursor-pointer rounded-sm focus-visible:outline-2 focus-visible:outline-primary">
            关于这些记录
          </summary>
          <p className="mt-2 max-w-2xl">
            仅来自已归档的本人说说，并非完整好友名单。“有来有回”指双方在同一段评论中发言，且有直接回复，每条说说只计一次。日期仅采用实际保存的对方互动时间，未保留日期时不展示。记录数量不能代表关系亲疏。
          </p>
          <p>更新于 {formatDate(data.directory.updatedAt)}</p>
        </details>
      )}
    </section>
  );
}
