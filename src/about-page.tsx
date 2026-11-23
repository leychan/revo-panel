import { useEffect, useState } from "react";
import type { MouseEvent } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { Activity, ExternalLink } from "lucide-react";
import config from "../src-tauri/tauri.conf.json";
import { native } from "./bridge";
import { useI18n } from "./i18n";

const repository = "https://github.com/leychan/revo-panel";
const aur = "https://aur.archlinux.org/packages/revo-panel-bin";
const author = "https://github.com/leychan";

export function AboutPage() {
  const { t } = useI18n();
  const [version, setVersion] = useState(config.version);
  const [linkError, setLinkError] = useState(false);

  useEffect(() => {
    // 桌面端优先使用运行中的包版本，浏览器预览使用构建配置。
    if (native) void getVersion().then(setVersion).catch(console.error);
  }, []);

  function openLink(event: MouseEvent<HTMLAnchorElement>, url: string) {
    if (!native) return;
    event.preventDefault();
    setLinkError(false);
    void invoke("open_about_link", { url }).catch(() => setLinkError(true));
  }

  return <section className="panel about-panel">
    <div className="about-heading">
      <span className="device-emblem"><Activity size={24} /></span>
      <div><h2>{config.productName}</h2><p>{t("版本")} <strong>{version}</strong></p></div>
    </div>
    <dl className="about-details">
      {[
        { label: "GitHub", url: repository, text: "github.com/leychan/revo-panel" },
        { label: "AUR", url: aur, text: "aur.archlinux.org/packages/revo-panel-bin" },
        { label: t("作者"), url: author, text: "leychan" },
      ].map(({ label, url, text }) => <div key={url}>
        <dt>{label}</dt>
        <dd><a href={url} target="_blank" rel="noreferrer" onClick={(event) => openLink(event, url)}>
          {text} <ExternalLink size={14} aria-hidden="true" />
        </a></dd>
      </div>)}
      <div><dt>{t("许可证")}</dt><dd>GPL-3.0-only</dd></div>
    </dl>
    {linkError && <p className="context-note" role="status">{t("无法打开系统浏览器")}</p>}
  </section>;
}
