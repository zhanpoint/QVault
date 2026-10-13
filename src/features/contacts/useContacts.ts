import { invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ArchiveItem } from "../archive/types";
import type { ContactDirectory, ContactFeedPage } from "./types";

const PAGE_SIZE = 20;

export function useContacts(ownerUin: string | null) {
  const available = isTauri();
  const [revision, setRevision] = useState(0);
  const [directory, setDirectory] = useState<ContactDirectory | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [selectedUin, setSelectedUin] = useState<string | null>(null);
  const [feeds, setFeeds] = useState<ArchiveItem[]>([]);
  const [feedsLoading, setFeedsLoading] = useState(false);
  const [feedsError, setFeedsError] = useState("");
  const [more, setMore] = useState(false);
  const sequence = useRef(0);
  const offset = useRef(0);
  const pending = useRef(false);

  useEffect(() => {
    let active = true;
    if (!ownerUin || !available) {
      setDirectory(null);
      setSelectedUin(null);
      setLoading(false);
      setError("");
      return;
    }
    setLoading(true);
    setError("");
    void invoke<ContactDirectory>("list_archive_contacts")
      .then((result) => {
        if (!active) return;
        setDirectory(result);
        setSelectedUin((current) =>
          result.contacts.some((contact) => contact.uin === current)
            ? current
            : null,
        );
      })
      .catch((reason) => {
        if (active) setError(String(reason));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [ownerUin, available, revision]);

  const snapshotId = directory?.snapshotId;
  const requestPage = useCallback(
    async (append: boolean, token: number) => {
      if (!ownerUin || !snapshotId || !selectedUin || pending.current) return;
      pending.current = true;
      setFeedsLoading(true);
      setFeedsError("");
      const start = append ? offset.current : 0;
      try {
        const page = await invoke<ContactFeedPage>("list_contact_feeds", {
          snapshotId,
          uin: selectedUin,
          offset: start,
          limit: PAGE_SIZE,
        });
        if (sequence.current !== token) return;
        setFeeds((current) =>
          append ? [...current, ...page.items] : page.items,
        );
        offset.current = start + PAGE_SIZE;
        setMore(offset.current < page.total);
      } catch (reason) {
        if (sequence.current === token) setFeedsError(String(reason));
      } finally {
        if (sequence.current === token) {
          pending.current = false;
          setFeedsLoading(false);
        }
      }
    },
    [ownerUin, snapshotId, selectedUin],
  );

  useEffect(() => {
    const token = ++sequence.current;
    offset.current = 0;
    pending.current = false;
    setFeeds([]);
    setMore(false);
    setFeedsLoading(false);
    setFeedsError("");
    void requestPage(false, token);
    return () => {
      sequence.current++;
    };
  }, [requestPage]);

  return {
    available,
    directory,
    loading,
    error,
    selectedUin,
    select: setSelectedUin,
    feeds,
    feedsLoading,
    feedsError,
    more,
    refresh: () => setRevision((current) => current + 1),
    loadMore: () => requestPage(true, sequence.current),
    retryFeeds: () => requestPage(offset.current > 0, sequence.current),
  };
}
