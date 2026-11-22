import { useEffect, useId, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent, ReactNode } from "react";
import { GripVertical } from "lucide-react";

const ids = ["power", "display", "gpu", "touchpad"] as const;
const columns = ["left", "right"] as const;
type CardId = typeof ids[number];
type Column = typeof columns[number];
type Layout = Record<Column, CardId[]>;
type Position = { column: Column; index: number };
type Card = { title: string; icon: ReactNode; description: string; status?: ReactNode; content: ReactNode };
type Drag = { source: CardId; pointer: number; x: number; y: number; moved: boolean; listeners: AbortController };
const storageKey = "revo-panel.system-card-columns.v1";
const oldStorageKey = "revo-panel.system-card-order.v1";

function isCardId(value: unknown): value is CardId {
  return typeof value === "string" && ids.some((id) => id === value);
}

function savedLayout(): Layout {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "null");
    if (saved && typeof saved === "object" && "left" in saved && "right" in saved &&
      Array.isArray(saved.left) && Array.isArray(saved.right)) {
      const left = [...new Set(saved.left.filter(isCardId))];
      const right = [...new Set(saved.right.filter(isCardId))].filter((id) => !left.includes(id));
      // 新增的卡片补到列尾，保留现有卡片的位置和用户留空的列。
      for (const id of ids) {
        if (!left.includes(id) && !right.includes(id)) (left.length <= right.length ? left : right).push(id);
      }
      return { left, right };
    }
  } catch { /* 新布局无法读取时尝试旧顺序。 */ }

  let order: readonly CardId[] = ids;
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(oldStorageKey) ?? "null");
    if (Array.isArray(saved)) {
      order = [...new Set([...saved.filter(isCardId), ...ids])];
    }
  } catch { /* 本地存储不可用时使用默认顺序。 */ }
  // 旧版只保存了总顺序，以从左到右的阅读顺序分配初始位置。
  return { left: order.filter((_, index) => index % 2 === 0), right: order.filter((_, index) => index % 2 === 1) };
}

export function SettingsGrid({ cards }: { cards: Record<CardId, Card> }) {
  const [layout, setLayout] = useState(savedLayout);
  const [preview, setPreview] = useState<{ source: CardId; target: Position | null; x: number; y: number } | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const grid = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const scrollFrame = useRef(0);
  const instructions = useId();

  useEffect(() => () => {
    drag.current?.listeners.abort();
    cancelAnimationFrame(scrollFrame.current);
  }, []);

  function move(source: CardId, target: Position, byKeyboard = false) {
    const next = { left: layout.left.filter((id) => id !== source), right: layout.right.filter((id) => id !== source) };
    const index = Math.min(Math.max(0, target.index), next[target.column].length);
    if (layout[target.column].indexOf(source) === index) return;
    next[target.column].splice(index, 0, source);
    setLayout(next);
    setAnnouncement(`${cards[source].title}已移动到${target.column === "left" ? "左" : "右"}列第 ${index + 1} 项`);
    try { localStorage.setItem(storageKey, JSON.stringify(next)); } catch { /* 当前页面仍可排序。 */ }
    // 跨列时卡片会在另一列重新挂载，等布局更新后再恢复把手焦点。
    requestAnimationFrame(() => grid.current?.querySelector<HTMLButtonElement>(`[data-settings-card="${source}"] .settings-grip`)
      ?.focus({ preventScroll: !byKeyboard }));
  }

  function targetAt(x: number, y: number, source: CardId): Position | null {
    const hit = document.elementFromPoint(x, y);
    const container = grid.current;
    if (!hit || !container?.contains(hit)) return null;
    const leftColumn = container.querySelector<HTMLElement>('[data-settings-column="left"]');
    const rightColumn = container.querySelector<HTMLElement>('[data-settings-column="right"]');
    if (!leftColumn || !rightColumn) return null;

    // 按整列区域判定落点，列尾、空列和两列之间也可以放下。
    const distanceTo = (element: HTMLElement) => {
      const { left, right, top, bottom } = element.getBoundingClientRect();
      return Math.hypot(Math.max(left - x, x - right, 0), Math.max(top - y, y - bottom, 0));
    };
    const column = distanceTo(leftColumn) <= distanceTo(rightColumn) ? "left" : "right";
    const destination = column === "left" ? leftColumn : rightColumn;
    let index = 0;
    for (const card of destination.querySelectorAll<HTMLElement>("[data-settings-card]")) {
      if (card.dataset.settingsCard === source) continue;
      const { top, height } = card.getBoundingClientRect();
      if (y < top + height / 2) break;
      index++;
    }
    return { column, index };
  }

  function updateTarget(current: Drag) {
    const target = targetAt(current.x, current.y, current.source);
    const staysInPlace = target && layout[target.column][target.index] === current.source;
    setPreview({ source: current.source, target: staysInPlace ? null : target, x: current.x, y: current.y });
  }

  function scrollNearEdge() {
    const current = drag.current;
    const workspace = grid.current?.closest<HTMLElement>(".workspace");
    if (!current || !workspace) return;
    const { top, bottom, left, right } = workspace.getBoundingClientRect();
    const lower = bottom - current.y;
    // 顶栏是固定的，拖到顶栏下方或视口底沿时才自动滚动。
    const upper = current.y - (workspace.querySelector(".topbar")?.getBoundingClientRect().bottom ?? top);
    const distance = lower < 44 ? Math.min(18, (44 - lower) / 3) : upper < 44 ? -Math.min(18, (44 - upper) / 3) : 0;
    if (distance && current.x >= left && current.x <= right && current.y >= top && current.y <= bottom) {
      const previousTop = workspace.scrollTop;
      workspace.scrollTop += distance;
      if (previousTop !== workspace.scrollTop) updateTarget(current);
    }
    scrollFrame.current = requestAnimationFrame(scrollNearEdge);
  }

  function startDrag(event: PointerEvent<HTMLDivElement>, source: CardId) {
    if (event.button !== 0 || drag.current) return;
    event.preventDefault();
    event.currentTarget.querySelector("button")?.focus({ preventScroll: true });
    const listeners = new AbortController();
    drag.current = { source, pointer: event.pointerId, x: event.clientX, y: event.clientY, moved: false, listeners };
    const options = { capture: true, signal: listeners.signal };
    // 在窗口监听移动和释放，指针离开卡片标题后仍能完成拖动。
    window.addEventListener("pointermove", dragMove, options);
    window.addEventListener("pointerup", drop, options);
    window.addEventListener("pointercancel", cancelPointer, options);
    window.addEventListener("blur", stopDrag, { signal: listeners.signal });
    window.addEventListener("keydown", cancelFromKeyboard, options);
  }

  function dragMove(event: globalThis.PointerEvent) {
    const current = drag.current;
    if (!current || current.pointer !== event.pointerId) return;
    if (event.pointerType === "mouse" && !(event.buttons & 1)) {
      stopDrag();
      return;
    }
    if (!current.moved && Math.hypot(current.x - event.clientX, current.y - event.clientY) < 4) return;
    current.moved = true;
    current.x = event.clientX;
    current.y = event.clientY;
    updateTarget(current);
    if (!scrollFrame.current) scrollFrame.current = requestAnimationFrame(scrollNearEdge);
  }

  function stopDrag() {
    drag.current?.listeners.abort();
    drag.current = null;
    cancelAnimationFrame(scrollFrame.current);
    scrollFrame.current = 0;
    setPreview(null);
  }

  function drop(event: globalThis.PointerEvent) {
    const current = drag.current;
    if (!current || current.pointer !== event.pointerId) return;
    const target = current.moved ? targetAt(event.clientX, event.clientY, current.source) : null;
    stopDrag();
    if (target) move(current.source, target);
  }

  function cancelPointer(event: globalThis.PointerEvent) {
    if (drag.current?.pointer === event.pointerId) stopDrag();
  }

  function cancelFromKeyboard(event: globalThis.KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    stopDrag();
  }

  function moveFromKeyboard(event: KeyboardEvent<HTMLButtonElement>, source: CardId, column: Column) {
    const offsets: Record<string, number> = { ArrowUp: -1, ArrowLeft: 0, ArrowDown: 1, ArrowRight: 0 };
    const offset = offsets[event.key];
    if (offset === undefined || drag.current) return;
    event.preventDefault();
    const destination = event.key === "ArrowLeft" ? "left" : event.key === "ArrowRight" ? "right" : column;
    move(source, { column: destination, index: layout[column].indexOf(source) + offset }, true);
  }

  return <div className="detail-grid settings-grid" ref={grid} data-sorting={preview ? true : undefined}>
    <span className="sr-only" id={instructions}>拖动标题可以换列或排序。按左右方向键换列，上下方向键在列内排序。</span>
    <span className="sr-only" role="status">{announcement}</span>
    {columns.map((column) => {
      const target = preview?.target?.column === column ? preview.target : null;
      const available = layout[column].filter((id) => id !== preview?.source);
      const before = target ? available[target.index] : undefined;
      const after = target && !before ? available.at(-1) : undefined;
      return <div className="settings-column" key={column} data-settings-column={column}
        data-sort-empty={target && available.length === 0 ? true : undefined}>
        {layout[column].map((id) => {
          const { title, icon, description, status, content } = cards[id];
          return <section className="panel control-panel settings-card" key={id} data-settings-card={id}
            data-sort-source={preview?.source === id || undefined}
            data-sort-insert={before === id ? "before" : after === id ? "after" : undefined}>
            <div className="section-heading" onPointerDown={(event) => startDrag(event, id)}>
              <div className="settings-card-title">
                <button className="settings-grip" type="button" aria-label={`移动${title}`} aria-describedby={instructions}
                  title="拖动或用方向键换列、排序" onKeyDown={(event) => moveFromKeyboard(event, id, column)}>
                  <GripVertical size={17} />
                </button>
                <div><h2>{icon}{title}</h2><p>{description}</p></div>
              </div>
              {status}
            </div>
            {content}
          </section>;
        })}
      </div>;
    })}
    {preview && <span className="settings-drag-label" aria-hidden="true"
      style={{ left: preview.x, top: preview.y }}><GripVertical size={16} />{cards[preview.source].title}</span>}
  </div>;
}
