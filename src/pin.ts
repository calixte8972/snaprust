import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";

import {
  closePin,
  getPinnedCapture,
  getPinnedCaptureImage,
  revealPinWindow,
  setPinOpacity,
  setPinWindowGeometry,
  warmupPinWindow,
} from "./screenshot";
import {
  clamp,
  minimumPinZoom,
  zoomedWindowGeometry,
  type PinPoint,
  type PinSize,
  type PinWorkArea,
} from "./pin-geometry";
import { waitForCompositorWarmup } from "./window-readiness";
import "./pin.css";

function requireElement<Element extends HTMLElement>(selector: string): Element {
  const element = document.querySelector<Element>(selector);
  if (!element) throw new Error(`required element is missing: ${selector}`);
  return element;
}

const pinWindow = getCurrentWindow();
const image = requireElement<HTMLImageElement>("#pin-image");
const hud = requireElement("#pin-hud");
let opacity = 1;
let zoom = 1;
let baseWidth = 1;
let baseHeight = 1;
let hudTimer: number | undefined;
let dragOrigin: Readonly<{ x: number; y: number }> | null = null;
let dragStarted = false;
let closing = false;
let imageObjectUrl: string | null = null;
let committedZoom = 1;
let zoomCommitInFlight = false;
let zoomRevision = 0;
let zoomAnchor = { x: 0.5, y: 0.5 };
let nativePosition: PinPoint | null = null;
let nativeSize: PinSize | null = null;
let nativeWorkArea: PinWorkArea | null = null;
const MIN_VISIBLE_EDGE = 64;

function showHud(message?: string): void {
  window.clearTimeout(hudTimer);
  hud.textContent = message ?? `${Math.round(zoom * 100)}% · 透明度 ${Math.round(opacity * 100)}%`;
  hud.classList.add("is-visible");
  hudTimer = window.setTimeout(() => hud.classList.remove("is-visible"), 1300);
}

function minimumZoom(): number {
  return minimumPinZoom(baseWidth, baseHeight, MIN_VISIBLE_EDGE);
}

function previewZoom(): void {
  const scale = zoom / committedZoom;
  image.style.transformOrigin = `${zoomAnchor.x * 100}% ${zoomAnchor.y * 100}%`;
  image.style.transform = scale === 1 ? "" : `scale(${scale})`;
}

async function refreshNativeGeometry(): Promise<void> {
  const [position, size, monitor] = await Promise.all([
    pinWindow.outerPosition(),
    pinWindow.innerSize(),
    currentMonitor(),
  ]);
  nativePosition = position;
  nativeSize = size;
  nativeWorkArea = monitor?.workArea ?? null;
}

async function commitZoom(revision: number): Promise<void> {
  if (closing) return;

  zoomCommitInFlight = true;
  const targetZoom = zoom;
  const startZoom = committedZoom;
  const targetAnchor = { ...zoomAnchor };
  try {
    if (!nativePosition || !nativeSize) await refreshNativeGeometry();
    if (!nativePosition || !nativeSize) throw new Error("pin window geometry is unavailable");
    const ratio = targetZoom / startZoom;
    const geometry = zoomedWindowGeometry(
      nativePosition,
      nativeSize,
      targetAnchor,
      ratio,
      nativeWorkArea,
      MIN_VISIBLE_EDGE,
    );

    await setPinWindowGeometry(
      pinWindow.label,
      geometry.position.x,
      geometry.position.y,
      geometry.size.width,
      geometry.size.height,
    );
    committedZoom = targetZoom;
    nativePosition = geometry.position;
    nativeSize = geometry.size;
  } catch (error) {
    if (revision === zoomRevision) zoom = committedZoom;
    console.error("failed to resize pin window", error);
    showHud(`缩放失败：${String(error)}`);
  } finally {
    zoomCommitInFlight = false;
    if (revision === zoomRevision) {
      image.style.transform = "";
    } else {
      previewZoom();
      void commitZoom(zoomRevision);
    }
  }
}

function scheduleZoomCommit(): void {
  zoomRevision += 1;
  if (!closing && !zoomCommitInFlight) void commitZoom(zoomRevision);
}

async function resetView(): Promise<void> {
  try {
    await refreshNativeGeometry();
    if (nativeSize) committedZoom = nativeSize.width / baseWidth;
    zoom = 1;
    opacity = 1;
    zoomAnchor = { x: 0.5, y: 0.5 };
    previewZoom();
    scheduleZoomCommit();
    scheduleOpacity(opacity);
    showHud();
  } catch (error) {
    console.error("failed to reset pin view", error);
    showHud(`重置失败：${String(error)}`);
  }
}

function createLatestScheduler<Value>(apply: (value: Value) => Promise<void>) {
  let pending: Value | undefined;
  let inFlight = false;
  let frame: number | undefined;

  const flush = (): void => {
    frame = undefined;
    if (inFlight || pending === undefined) return;
    const value = pending;
    pending = undefined;
    inFlight = true;
    void apply(value).catch((error) => {
      console.error("pin window operation failed", error);
      showHud(`操作失败：${String(error)}`);
    }).finally(() => {
      inFlight = false;
      if (pending !== undefined && frame === undefined) {
        frame = window.requestAnimationFrame(flush);
      }
    });
  };

  return (value: Value): void => {
    pending = value;
    if (!inFlight && frame === undefined) {
      frame = window.requestAnimationFrame(flush);
    }
  };
}

async function close(): Promise<void> {
  if (closing) return;
  closing = true;
  try {
    await closePin(pinWindow.label);
  } catch (error) {
    closing = false;
    console.error("failed to close pin window", error);
    showHud(`关闭失败：${String(error)}`);
  }
}

const scheduleOpacity = createLatestScheduler((nextOpacity: number) =>
  setPinOpacity(pinWindow.label, nextOpacity)
);

document.addEventListener("contextmenu", (event) => event.preventDefault());

document.addEventListener("pointerdown", (event) => {
  if (event.button !== 0) return;
  dragOrigin = { x: event.clientX, y: event.clientY };
  dragStarted = false;
});

document.addEventListener("pointermove", (event) => {
  if (!dragOrigin || dragStarted || (event.buttons & 1) === 0) return;
  if (Math.hypot(event.clientX - dragOrigin.x, event.clientY - dragOrigin.y) < 4) return;
  dragStarted = true;
  void pinWindow.startDragging()
    .then(refreshNativeGeometry)
    .catch((error) => {
      console.error("failed to start dragging pin window", error);
      showHud(`拖动失败：${String(error)}`);
    });
});

document.addEventListener("pointerup", () => {
  dragOrigin = null;
  dragStarted = false;
  void refreshNativeGeometry().catch((error) => {
    console.error("failed to refresh pin geometry after dragging", error);
  });
});

document.addEventListener("dblclick", (event) => {
  if (event.button === 0) void close();
});

document.addEventListener("wheel", (event) => {
  event.preventDefault();
  if (event.shiftKey) {
    opacity = Math.max(0.2, Math.min(1, opacity + (event.deltaY < 0 ? 0.05 : -0.05)));
    scheduleOpacity(opacity);
  } else {
    const delta = clamp(-event.deltaY / 100, -4, 4);
    zoom = clamp(zoom * Math.pow(1.1, delta), minimumZoom(), 5);
    zoomAnchor = {
      x: clamp(event.clientX / Math.max(1, window.innerWidth), 0, 1),
      y: clamp(event.clientY / Math.max(1, window.innerHeight), 0, 1),
    };
    previewZoom();
    scheduleZoomCommit();
  }
  showHud();
}, { passive: false });

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    event.preventDefault();
    void close();
  } else if (event.key === "0") {
    event.preventDefault();
    void resetView();
  } else if (event.key === "[" || event.key === "]") {
    event.preventDefault();
    opacity = Math.max(0.2, Math.min(1, opacity + (event.key === "]" ? 0.05 : -0.05)));
    scheduleOpacity(opacity);
    showHud();
  }
});

async function initialize(): Promise<void> {
  const totalStarted = performance.now();
  const ipcStarted = performance.now();
  const [metadata, png] = await Promise.all([
    getPinnedCapture(pinWindow.label),
    getPinnedCaptureImage(pinWindow.label),
  ]);
  const ipcMs = performance.now() - ipcStarted;
  const initialScale = Math.min(960 / metadata.width, 720 / metadata.height, 1);
  baseWidth = Math.max(1, metadata.width * initialScale);
  baseHeight = Math.max(1, metadata.height * initialScale);
  imageObjectUrl = URL.createObjectURL(new Blob([png], { type: "image/png" }));
  image.src = imageObjectUrl;
  const decodeStarted = performance.now();
  await image.decode();
  const decodeMs = performance.now() - decodeStarted;
  await refreshNativeGeometry();
  await warmupPinWindow(pinWindow.label);
  await waitForCompositorWarmup();
  await revealPinWindow(pinWindow.label);
  const totalMs = performance.now() - totalStarted;
  console.info(
    `[SnapRust performance] pin-load: ipc=${ipcMs.toFixed(1)}ms · decode=${decodeMs.toFixed(1)}ms · total=${totalMs.toFixed(1)}ms`,
  );
  showHud(`载入 ${totalMs.toFixed(0)}ms · 滚轮缩放 · Shift+滚轮透明度 · 双击关闭`);
}

window.addEventListener("beforeunload", () => {
  if (imageObjectUrl) URL.revokeObjectURL(imageObjectUrl);
});

initialize().catch(async (error) => {
  console.error("failed to initialize pin window", error);
  try {
    await close();
  } catch (cleanupError) {
    console.error("failed to clean up an incomplete pin window", cleanupError);
  }
});
