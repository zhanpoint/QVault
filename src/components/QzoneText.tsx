import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
import { memo, useEffect, useState } from "react";

function Emoticon({ code }: { code: string }) {
  const [source, setSource] = useState("");
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let cancelled = false;
    const load = isTauri()
      ? invoke<string>("load_qzone_emoticon", { code }).then(convertFileSrc)
      : Promise.resolve(`https://qzonestyle.gtimg.cn/qzone/em/${code}.gif`);
    void load
      .then((url) => {
        if (!cancelled) setSource(url);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [code]);
  return failed ? (
    <span title={`QQ 表情 ${code}`}>〔表情〕</span>
  ) : source ? (
    <img
      src={source}
      alt="QQ 表情"
      title={`QQ 表情 ${code}`}
      className="mx-0.5 inline-block size-[1.5em] align-[-0.3em] object-contain"
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
    />
  ) : (
    <span
      aria-label="正在读取 QQ 表情"
      aria-busy="true"
      className="mx-0.5 inline-block size-[1.5em] align-[-0.3em] rounded bg-muted"
    />
  );
}

/** Render Qzone's structured emoticon tokens while keeping all other text escaped. */
export const QzoneText = memo(function QzoneText({ text }: { text: string }) {
  return (
    <>
      {text.split(/(\[em\]e\d+\[\/em\])/g).map((part, index) => {
        const match = /^\[em\](e\d+)\[\/em\]$/.exec(part);
        return match ? (
          <Emoticon key={`${index}:${match[1]}`} code={match[1]} />
        ) : (
          part
        );
      })}
    </>
  );
});
