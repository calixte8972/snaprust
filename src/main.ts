import { listen } from "@tauri-apps/api/event";

import {
  cancelCapture,
  copySelectedCapture,
  getCurrentCapture,
  getCurrentCaptureImage,
  getSelectedCaptureImage,
  pinSelectedCapture,
  revealCaptureOverlay,
  selectCaptureRegion,
  setCaptureAnnotations,
  type Annotation,
  type AnnotationPoint,
  type CapturePayload,
  type SelectionPayload,
} from "./screenshot";
import {
  normalizeSelection,
  selectionToPhysicalPixels,
  selectionHasArea,
  simplifyPolyline,
  type Point,
  type SelectionRect,
} from "./selection";
import { mosaicRgbaPixels } from "./image-processing";
import { waitForCompositorWarmup } from "./window-readiness";
import "./style.css";

function requireElement<Element extends HTMLElement>(selector: string): Element {
  const element = document.querySelector<Element>(selector);
  if (!element) {
    throw new Error(`required element is missing: ${selector}`);
  }

  return element;
}

const overlay = requireElement("#capture-overlay");
const captureImage = requireElement<HTMLImageElement>("#capture-image");
const captureStatus = requireElement("#capture-status");
const selectionBox = requireElement("#selection-box");
const selectionSize = requireElement("#selection-size");
const monitorGuides = requireElement("#monitor-guides");
const captureDesktop = requireElement("#capture-desktop");
const capturePointer = requireElement("#capture-pointer");
const annotationEditor = requireElement("#annotation-editor");
const annotationCanvasWrap = requireElement("#annotation-canvas-wrap");
const annotationCanvas = requireElement<HTMLCanvasElement>("#annotation-canvas");
const annotationTextEditor = requireElement("#annotation-text-editor");
const annotationTextInput = requireElement<HTMLTextAreaElement>("#annotation-text-input");
const annotationTextConfirm = requireElement<HTMLButtonElement>("#annotation-text-confirm");
const annotationTextCancel = requireElement<HTMLButtonElement>("#annotation-text-cancel");
const annotationStatus = requireElement("#annotation-status");
const annotationColor = requireElement<HTMLInputElement>("#annotation-color");
const annotationWidth = requireElement<HTMLInputElement>("#annotation-width");
const annotationWidthValue = requireElement("#annotation-width-value");
const annotationUndo = requireElement<HTMLButtonElement>("#annotation-undo");
const annotationRedo = requireElement<HTMLButtonElement>("#annotation-redo");
const annotationClear = requireElement<HTMLButtonElement>("#annotation-clear");
const annotationPin = requireElement<HTMLButtonElement>("#annotation-pin");
const annotationContextOrNull = annotationCanvas.getContext("2d", { willReadFrequently: true });
if (!annotationContextOrNull) {
  throw new Error("2D canvas context is unavailable");
}
const annotationContext: CanvasRenderingContext2D = annotationContextOrNull;
const committedAnnotationCanvas = document.createElement("canvas");
const committedAnnotationContextOrNull = committedAnnotationCanvas.getContext("2d", {
  willReadFrequently: true,
});
if (!committedAnnotationContextOrNull) {
  throw new Error("offscreen 2D canvas context is unavailable");
}
const committedAnnotationContext: CanvasRenderingContext2D = committedAnnotationContextOrNull;

let dragStart: Point | null = null;
let activePointerId: number | null = null;
let currentSelection: SelectionRect | null = null;
let selectedCapture: SelectionPayload | null = null;
let captureReady = false;
let copyInProgress = false;
let pinInProgress = false;
let currentCapture: CapturePayload | null = null;
type AnnotationTool = "arrow" | "rectangle" | "ellipse" | "brush" | "mosaic" | "text";
let annotationTool: AnnotationTool = "arrow";
let selectedImage: HTMLImageElement | null = null;
let annotations: Annotation[] = [];
let annotationRedoStack: Annotation[][] = [];
let annotationStart: AnnotationPoint | null = null;
let annotationDraft: Annotation | null = null;
let annotationPointerId: number | null = null;
let annotationRenderFrame: number | null = null;
let captureImageObjectUrl: string | null = null;
let selectedImageObjectUrl: string | null = null;
let captureSessionVersion = 0;
const MAX_BRUSH_POINTS = 20_000;
let pendingTextAnnotation: Readonly<{
  position: AnnotationPoint;
  color: string;
  fontSize: number;
}> | null = null;
let annotationPerformanceSummary = "";

function formatMilliseconds(value: number): string {
  return `${value.toFixed(value < 10 ? 1 : 0)}ms`;
}

function reportPerformance(stage: string, metrics: Readonly<Record<string, number>>): void {
  const summary = Object.entries(metrics)
    .map(([label, value]) => `${label}=${formatMilliseconds(value)}`)
    .join(" · ");
  console.info(`[SnapRust performance] ${stage}: ${summary}`);
}

function pointFromEvent(event: PointerEvent): Point {
  return { x: event.clientX, y: event.clientY };
}

function revokeObjectUrl(url: string | null): void {
  if (url) URL.revokeObjectURL(url);
}

function releaseCapturePreview(): void {
  revokeObjectUrl(captureImageObjectUrl);
  captureImageObjectUrl = null;
  captureImage.removeAttribute("src");
}

function releaseImageResources(): void {
  releaseCapturePreview();
  revokeObjectUrl(selectedImageObjectUrl);
  selectedImageObjectUrl = null;
  selectedImage = null;

  if (annotationRenderFrame !== null) {
    window.cancelAnimationFrame(annotationRenderFrame);
    annotationRenderFrame = null;
  }
  annotationCanvas.width = 1;
  annotationCanvas.height = 1;
  committedAnnotationCanvas.width = 1;
  committedAnnotationCanvas.height = 1;
}

function resetSelectionUi(): void {
  captureSessionVersion += 1;
  releaseImageResources();
  dragStart = null;
  activePointerId = null;
  currentSelection = null;
  selectedCapture = null;
  copyInProgress = false;
  pinInProgress = false;
  overlay.dataset.hasSelection = "false";
  selectionBox.style.cssText = "";
  selectionSize.textContent = "";
  annotationEditor.hidden = true;
  overlay.dataset.state = "idle";
  annotations = [];
  annotationRedoStack = [];
  annotationStart = null;
  annotationDraft = null;
  annotationPointerId = null;
  annotationUndo.disabled = true;
  annotationRedo.disabled = true;
  annotationClear.disabled = true;
  annotationPin.disabled = false;
  annotationPerformanceSummary = "";
  cancelTextEditor();
}

function currentAnnotationWidth(): number {
  return Number(annotationWidth.value);
}

function setAnnotationTool(tool: AnnotationTool): void {
  if (tool !== "text") cancelTextEditor();
  annotationTool = tool;
  document.querySelectorAll<HTMLButtonElement>("[data-tool]").forEach((button) => {
    const active = button.dataset.tool === tool;
    button.classList.toggle("is-active", active);
    button.setAttribute("aria-pressed", String(active));
  });
  annotationCanvas.style.cursor = tool === "text" ? "text" : "crosshair";
}

function cancelTextEditor(): void {
  pendingTextAnnotation = null;
  annotationTextInput.value = "";
  annotationTextEditor.hidden = true;
}

function commitTextEditor(): boolean {
  const pending = pendingTextAnnotation;
  const text = annotationTextInput.value.trim();
  cancelTextEditor();
  if (!pending || !text) return false;
  commitAnnotation({
    kind: "text",
    position: pending.position,
    text,
    color: pending.color,
    fontSize: pending.fontSize,
  });
  return true;
}

function openTextEditor(position: AnnotationPoint): void {
  cancelTextEditor();
  const canvasBounds = annotationCanvas.getBoundingClientRect();
  const wrapBounds = annotationCanvasWrap.getBoundingClientRect();
  const scale = canvasBounds.width / annotationCanvas.width;
  const fontSize = Math.max(16, currentAnnotationWidth() * 5);
  const editorWidth = Math.min(320, Math.max(180, annotationCanvasWrap.clientWidth - 24));
  const desiredLeft =
    canvasBounds.left - wrapBounds.left + annotationCanvasWrap.scrollLeft + position.x * scale;
  const desiredTop =
    canvasBounds.top - wrapBounds.top + annotationCanvasWrap.scrollTop + position.y * scale;
  const visibleLeft = annotationCanvasWrap.scrollLeft + 8;
  const visibleTop = annotationCanvasWrap.scrollTop + 8;
  const maximumLeft =
    annotationCanvasWrap.scrollLeft + annotationCanvasWrap.clientWidth - editorWidth - 8;
  const maximumTop =
    annotationCanvasWrap.scrollTop + annotationCanvasWrap.clientHeight - 142;

  pendingTextAnnotation = {
    position,
    color: annotationColor.value,
    fontSize,
  };
  annotationTextEditor.style.left = `${Math.max(visibleLeft, Math.min(maximumLeft, desiredLeft))}px`;
  annotationTextEditor.style.top = `${Math.max(visibleTop, Math.min(maximumTop, desiredTop))}px`;
  annotationTextEditor.style.width = `${editorWidth}px`;
  annotationTextInput.style.color = annotationColor.value;
  annotationTextInput.style.fontSize = `${Math.max(14, fontSize * scale)}px`;
  annotationTextEditor.hidden = false;
  window.requestAnimationFrame(() => annotationTextInput.focus());
}

function annotationPointFromEvent(event: PointerEvent): AnnotationPoint {
  const bounds = annotationCanvas.getBoundingClientRect();
  return {
    x: Math.max(0, Math.min(annotationCanvas.width, ((event.clientX - bounds.left) * annotationCanvas.width) / bounds.width)),
    y: Math.max(0, Math.min(annotationCanvas.height, ((event.clientY - bounds.top) * annotationCanvas.height) / bounds.height)),
  };
}

function annotationRect(start: AnnotationPoint, end: AnnotationPoint) {
  return {
    x: Math.min(start.x, end.x),
    y: Math.min(start.y, end.y),
    width: Math.abs(end.x - start.x),
    height: Math.abs(end.y - start.y),
  };
}

function drawMosaic(
  context: CanvasRenderingContext2D,
  rect: Readonly<{ x: number; y: number; width: number; height: number }>,
  blockSize: number,
): void {
  const left = Math.max(0, Math.floor(rect.x));
  const top = Math.max(0, Math.floor(rect.y));
  const right = Math.min(context.canvas.width, Math.ceil(rect.x + rect.width));
  const bottom = Math.min(context.canvas.height, Math.ceil(rect.y + rect.height));
  const width = right - left;
  const height = bottom - top;
  if (width <= 0 || height <= 0) return;

  const pixels = context.getImageData(left, top, width, height);
  mosaicRgbaPixels(pixels.data, width, height, blockSize);
  context.putImageData(pixels, left, top);
}

function drawAnnotation(annotation: Annotation, context: CanvasRenderingContext2D): void {
  context.save();
  switch (annotation.kind) {
    case "arrow": {
      const angle = Math.atan2(annotation.end.y - annotation.start.y, annotation.end.x - annotation.start.x);
      const head = Math.max(12, annotation.width * 4.5);
      context.strokeStyle = annotation.color;
      context.lineWidth = annotation.width;
      context.lineCap = "round";
      context.lineJoin = "round";
      context.beginPath();
      context.moveTo(annotation.start.x, annotation.start.y);
      context.lineTo(annotation.end.x, annotation.end.y);
      for (const offset of [Math.PI * 0.78, -Math.PI * 0.78]) {
        context.moveTo(annotation.end.x, annotation.end.y);
        context.lineTo(annotation.end.x + head * Math.cos(angle + offset), annotation.end.y + head * Math.sin(angle + offset));
      }
      context.stroke();
      break;
    }
    case "rectangle":
      context.strokeStyle = annotation.color;
      context.lineWidth = annotation.width;
      context.strokeRect(annotation.rect.x, annotation.rect.y, annotation.rect.width, annotation.rect.height);
      break;
    case "ellipse": {
      context.strokeStyle = annotation.color;
      context.lineWidth = annotation.width;
      context.beginPath();
      context.ellipse(annotation.rect.x + annotation.rect.width / 2, annotation.rect.y + annotation.rect.height / 2, annotation.rect.width / 2, annotation.rect.height / 2, 0, 0, Math.PI * 2);
      context.stroke();
      break;
    }
    case "brush":
      context.strokeStyle = annotation.color;
      context.lineWidth = annotation.width;
      context.lineCap = "round";
      context.lineJoin = "round";
      context.beginPath();
      annotation.points.forEach((point, index) => {
        if (index === 0) context.moveTo(point.x, point.y);
        else context.lineTo(point.x, point.y);
      });
      context.stroke();
      break;
    case "mosaic":
      drawMosaic(context, annotation.rect, annotation.blockSize);
      break;
    case "text":
      context.fillStyle = annotation.color;
      context.font = `${annotation.fontSize}px "Microsoft YaHei", "Segoe UI", sans-serif`;
      context.textBaseline = "top";
      annotation.text.split("\n").forEach((line, index) => context.fillText(line, annotation.position.x, annotation.position.y + index * annotation.fontSize * 1.25));
      break;
  }
  context.restore();
}

function updateAnnotationStatus(): void {
  const performanceText = annotationPerformanceSummary
    ? ` · ${annotationPerformanceSummary}`
    : "";
  annotationStatus.textContent = `${annotationCanvas.width} × ${annotationCanvas.height} · ${annotations.length} 个标注 · Rust 将在复制时合成${performanceText}`;
  annotationUndo.disabled = annotations.length === 0;
  annotationRedo.disabled = annotationRedoStack.length === 0;
  annotationClear.disabled = annotations.length === 0;
}

function renderAnnotationCanvas(): void {
  annotationRenderFrame = null;
  if (!selectedImage) return;
  annotationContext.clearRect(0, 0, annotationCanvas.width, annotationCanvas.height);
  annotationContext.drawImage(committedAnnotationCanvas, 0, 0);
  if (annotationDraft) drawAnnotation(annotationDraft, annotationContext);
}

function scheduleAnnotationRender(): void {
  if (annotationRenderFrame === null) {
    annotationRenderFrame = window.requestAnimationFrame(renderAnnotationCanvas);
  }
}

function rebuildCommittedAnnotationCanvas(): void {
  if (!selectedImage) return;
  if (annotationRenderFrame !== null) {
    window.cancelAnimationFrame(annotationRenderFrame);
    annotationRenderFrame = null;
  }
  committedAnnotationContext.clearRect(
    0,
    0,
    committedAnnotationCanvas.width,
    committedAnnotationCanvas.height,
  );
  committedAnnotationContext.drawImage(
    selectedImage,
    0,
    0,
    committedAnnotationCanvas.width,
    committedAnnotationCanvas.height,
  );
  annotations.forEach((annotation) => drawAnnotation(annotation, committedAnnotationContext));
  updateAnnotationStatus();
  renderAnnotationCanvas();
}

function commitAnnotation(annotation: Annotation): void {
  annotations = [...annotations, annotation];
  annotationRedoStack = [];
  annotationDraft = null;
  rebuildCommittedAnnotationCanvas();
}

async function openAnnotationEditor(selected: SelectionPayload): Promise<Readonly<{
  pngIpcMs: number;
  decodeMs: number;
  totalMs: number;
}> | null> {
  const totalStarted = performance.now();
  const version = captureSessionVersion;
  const pngStarted = performance.now();
  const png = await getSelectedCaptureImage();
  const pngIpcMs = performance.now() - pngStarted;
  if (version !== captureSessionVersion) return null;

  const objectUrl = URL.createObjectURL(new Blob([png], { type: "image/png" }));
  const image = new Image();
  image.src = objectUrl;
  const decodeStarted = performance.now();
  try {
    await image.decode();
  } catch (error) {
    URL.revokeObjectURL(objectUrl);
    throw error;
  }
  if (version !== captureSessionVersion) {
    URL.revokeObjectURL(objectUrl);
    return null;
  }

  revokeObjectUrl(selectedImageObjectUrl);
  selectedImageObjectUrl = objectUrl;
  selectedImage = image;
  releaseCapturePreview();
  annotationCanvas.width = selected.width;
  annotationCanvas.height = selected.height;
  committedAnnotationCanvas.width = selected.width;
  committedAnnotationCanvas.height = selected.height;
  annotations = [];
  annotationRedoStack = [];
  annotationEditor.hidden = false;
  overlay.dataset.state = "editing";
  rebuildCommittedAnnotationCanvas();
  return {
    pngIpcMs,
    decodeMs: performance.now() - decodeStarted,
    totalMs: performance.now() - totalStarted,
  };
}

function toPhysicalPoint(point: Point): Point | null {
  if (!currentCapture) {
    return null;
  }

  const physical = selectionToPhysicalPixels(
    {
      x: point.x,
      y: point.y,
      width: 0,
      height: 0,
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
    },
    currentCapture.width,
    currentCapture.height,
    currentCapture.desktop.x,
    currentCapture.desktop.y,
  );
  return { x: physical.x, y: physical.y };
}

function monitorAt(point: Point): CapturePayload["desktop"]["monitors"][number] | undefined {
  return currentCapture?.desktop.monitors.find(
    (monitor) =>
      point.x >= monitor.x &&
      point.x < monitor.x + monitor.width &&
      point.y >= monitor.y &&
      point.y < monitor.y + monitor.height,
  );
}

function updatePointerReadout(point: Point): void {
  const physical = toPhysicalPoint(point);
  if (!physical) {
    capturePointer.textContent = "坐标：等待截图";
    return;
  }

  const monitor = monitorAt(physical);
  const monitorText = monitor
    ? `显示器 ${monitor.index}${monitor.isPrimary ? "（主）" : ""} · ${Math.round(monitor.scaleFactor * 100)}%`
    : "显示器：未知";
  capturePointer.textContent = `逻辑：${Math.round(point.x)}, ${Math.round(point.y)} · 物理：${physical.x}, ${physical.y} · ${monitorText}`;
}

function renderMonitorGuides(capture: CapturePayload): void {
  monitorGuides.replaceChildren();
  for (const monitor of capture.desktop.monitors) {
    const guide = document.createElement("div");
    guide.className = "monitor-guide";
    guide.style.left = `${((monitor.x - capture.desktop.x) / capture.desktop.width) * 100}%`;
    guide.style.top = `${((monitor.y - capture.desktop.y) / capture.desktop.height) * 100}%`;
    guide.style.width = `${(monitor.width / capture.desktop.width) * 100}%`;
    guide.style.height = `${(monitor.height / capture.desktop.height) * 100}%`;
    guide.textContent = `显示器 ${monitor.index}${monitor.isPrimary ? " · 主" : ""} · ${Math.round(monitor.scaleFactor * 100)}%`;
    monitorGuides.append(guide);
  }
}

function physicalSelectionSize(selection: SelectionRect): { width: number; height: number } | null {
  if (!currentCapture) {
    return null;
  }

  const physical = selectionToPhysicalPixels(
    selection,
    currentCapture.width,
    currentCapture.height,
    currentCapture.desktop.x,
    currentCapture.desktop.y,
  );
  return { width: physical.width, height: physical.height };
}

function renderSelection(selection: SelectionRect): void {
  overlay.dataset.hasSelection = "true";
  overlay.style.setProperty("--selection-x", `${selection.x}px`);
  overlay.style.setProperty("--selection-y", `${selection.y}px`);
  overlay.style.setProperty("--selection-width", `${selection.width}px`);
  overlay.style.setProperty("--selection-height", `${selection.height}px`);
  selectionBox.style.left = `${selection.x}px`;
  selectionBox.style.top = `${selection.y}px`;
  selectionBox.style.width = `${selection.width}px`;
  selectionBox.style.height = `${selection.height}px`;
  const physical = physicalSelectionSize(selection);
  selectionSize.textContent = physical
    ? `逻辑 ${Math.round(selection.width)} × ${Math.round(selection.height)} · 像素 ${physical.width} × ${physical.height}`
    : `${Math.round(selection.width)} × ${Math.round(selection.height)}`;
}

function createSelection(start: Point, end: Point): SelectionRect {
  return normalizeSelection(start, end, window.innerWidth, window.innerHeight);
}

async function cancel(): Promise<void> {
  overlay.dataset.state = "idle";
  resetSelectionUi();

  try {
    await cancelCapture();
  } catch (error) {
    console.error("failed to cancel capture mode", error);
  }
}

async function commitSelection(selection: SelectionRect): Promise<void> {
  const totalStarted = performance.now();
  captureStatus.textContent = "正在生成选区…";

  try {
    if (!currentCapture) throw new Error("capture metadata is unavailable");
    const physicalSelection = selectionToPhysicalPixels(
      selection,
      currentCapture.width,
      currentCapture.height,
      currentCapture.desktop.x,
      currentCapture.desktop.y,
    );
    const cropRoundTripStarted = performance.now();
    const selected = await selectCaptureRegion(physicalSelection);
    const cropRoundTripMs = performance.now() - cropRoundTripStarted;
    if (currentSelection !== selection) {
      return;
    }

    selectedCapture = selected;
    captureStatus.textContent = `${selected.width} × ${selected.height} · Ctrl+C 复制 · Esc 取消`;
    const editorPerformance = await openAnnotationEditor(selected);
    if (!editorPerformance) return;
    annotationPerformanceSummary = `性能：裁剪 ${formatMilliseconds(selected.cropMs)} · 选区 PNG/IPC ${formatMilliseconds(editorPerformance.pngIpcMs)} · 解码 ${formatMilliseconds(editorPerformance.decodeMs)}`;
    updateAnnotationStatus();
    reportPerformance("selection", {
      rustCrop: selected.cropMs,
      cropRoundTrip: cropRoundTripMs,
      selectedPngIpc: editorPerformance.pngIpcMs,
      browserDecode: editorPerformance.decodeMs,
      total: performance.now() - totalStarted,
    });
  } catch (error) {
    selectedCapture = null;
    captureStatus.textContent = "选区生成失败，请重新拖动或按 Esc 取消";
    console.error("failed to crop capture selection", error);
  }
}

async function copySelection(): Promise<void> {
  commitTextEditor();
  if (!selectedCapture || copyInProgress || pinInProgress) {
    return;
  }

  copyInProgress = true;
  annotationPin.disabled = true;
  const totalStarted = performance.now();
  let annotationSyncMs = 0;
  captureStatus.textContent = "正在复制到剪贴板…";
  if (!annotationEditor.hidden) annotationStatus.textContent = "正在由 Rust 合成并复制…";

  try {
    if (!annotationEditor.hidden && annotations.length > 0) {
      const annotationStarted = performance.now();
      await setCaptureAnnotations(annotations);
      annotationSyncMs = performance.now() - annotationStarted;
    }
    const copied = await copySelectedCapture();
    reportPerformance("copy", {
      annotationSync: annotationSyncMs,
      rustRender: copied.renderMs,
      clipboard: copied.clipboardMs,
      rustTotal: copied.totalMs,
      endToEnd: performance.now() - totalStarted,
    });
    captureStatus.textContent = `已复制 ${copied.width} × ${copied.height}`;
    resetSelectionUi();
  } catch (error) {
    captureStatus.textContent = "复制失败，请重试或按 Esc 取消";
    if (!annotationEditor.hidden) annotationStatus.textContent = `复制失败：${String(error)}`;
    console.error("failed to copy selected capture", error);
  } finally {
    copyInProgress = false;
    annotationPin.disabled = false;
  }
}

async function pinSelection(): Promise<void> {
  if (!selectedCapture || pinInProgress || copyInProgress) {
    return;
  }

  pinInProgress = true;
  annotationPin.disabled = true;
  const totalStarted = performance.now();
  let annotationSyncMs = 0;
  captureStatus.textContent = "正在创建钉图…";
  annotationStatus.textContent = "正在由 Rust 合成并创建置顶窗口…";

  try {
    if (annotations.length > 0) {
      const annotationStarted = performance.now();
      await setCaptureAnnotations(annotations);
      annotationSyncMs = performance.now() - annotationStarted;
    }
    const pinned = await pinSelectedCapture();
    reportPerformance("pin", {
      annotationSync: annotationSyncMs,
      rustRender: pinned.renderMs,
      pngEncode: pinned.pngEncodeMs,
      windowCreate: pinned.windowCreateMs,
      rustTotal: pinned.totalMs,
      endToEnd: performance.now() - totalStarted,
    });
    captureStatus.textContent = `已钉住 ${pinned.width} × ${pinned.height}`;
    resetSelectionUi();
  } catch (error) {
    captureStatus.textContent = "钉图失败，请重试或按 Esc 取消";
    annotationStatus.textContent = `钉图失败：${String(error)}`;
    console.error("failed to pin selected capture", error);
  } finally {
    pinInProgress = false;
    annotationPin.disabled = false;
  }
}

window.addEventListener("keydown", (event) => {
  if (annotationTextEditor.contains(event.target as Node)) return;

  if (event.key === "Escape") {
    event.preventDefault();
    if (pendingTextAnnotation) {
      cancelTextEditor();
      return;
    }
    void cancel();
    return;
  }

  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "c") {
    event.preventDefault();
    void copySelection();
    return;
  }

  if (!annotationEditor.hidden && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z") {
    event.preventDefault();
    if (annotations.length > 0) {
      annotationRedoStack.push(annotations);
      annotations = annotations.slice(0, -1);
      rebuildCommittedAnnotationCanvas();
    }
    return;
  }

  if (!annotationEditor.hidden && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "y") {
    event.preventDefault();
    const restored = annotationRedoStack.pop();
    if (restored) {
      annotations = restored;
      rebuildCommittedAnnotationCanvas();
    }
    return;
  }

  if (event.key === "Enter") {
    event.preventDefault();
    void copySelection();
  }
});

window.addEventListener("contextmenu", (event) => {
  if (annotationTextEditor.contains(event.target as Node)) return;
  event.preventDefault();
  void cancel();
});

overlay.addEventListener("pointerdown", (event) => {
  if (!captureReady || !annotationEditor.hidden || event.button !== 0 || copyInProgress) {
    return;
  }

  dragStart = pointFromEvent(event);
  activePointerId = event.pointerId;
  selectedCapture = null;
  currentSelection = createSelection(dragStart, dragStart);
  updatePointerReadout(dragStart);
  overlay.setPointerCapture(event.pointerId);
  renderSelection(currentSelection);
  event.preventDefault();
});

overlay.addEventListener("pointermove", (event) => {
  if (dragStart === null || event.pointerId !== activePointerId) {
    return;
  }

  currentSelection = createSelection(dragStart, pointFromEvent(event));
  renderSelection(currentSelection);
  event.preventDefault();
});

overlay.addEventListener("pointerup", (event) => {
  if (dragStart === null || event.pointerId !== activePointerId) {
    return;
  }

  const selection = createSelection(dragStart, pointFromEvent(event));
  updatePointerReadout(pointFromEvent(event));
  dragStart = null;
  activePointerId = null;
  if (overlay.hasPointerCapture(event.pointerId)) {
    overlay.releasePointerCapture(event.pointerId);
  }

  currentSelection = selection;
  renderSelection(selection);
  if (!selectionHasArea(selection)) {
    selectedCapture = null;
    captureStatus.textContent = "选区太小，请重新拖动";
    event.preventDefault();
    return;
  }

  void commitSelection(selection);
  event.preventDefault();
});

overlay.addEventListener("pointercancel", () => {
  dragStart = null;
  activePointerId = null;
  currentSelection = null;
  selectedCapture = null;
  overlay.dataset.hasSelection = "false";
  captureStatus.textContent = "选择已取消，请重新拖动";
});

overlay.addEventListener("pointermove", (event) => {
  updatePointerReadout(pointFromEvent(event));
});

document.querySelectorAll<HTMLButtonElement>("[data-tool]").forEach((button) => {
  button.addEventListener("click", () => setAnnotationTool(button.dataset.tool as AnnotationTool));
});

annotationWidth.addEventListener("input", () => {
  annotationWidthValue.textContent = `${annotationWidth.value} px`;
});

annotationUndo.addEventListener("click", () => {
  if (annotations.length === 0) return;
  annotationRedoStack.push(annotations);
  annotations = annotations.slice(0, -1);
  rebuildCommittedAnnotationCanvas();
});

annotationRedo.addEventListener("click", () => {
  const restored = annotationRedoStack.pop();
  if (!restored) return;
  annotations = restored;
  rebuildCommittedAnnotationCanvas();
});

annotationClear.addEventListener("click", () => {
  if (annotations.length === 0) return;
  annotationRedoStack.push(annotations);
  annotations = [];
  rebuildCommittedAnnotationCanvas();
});

annotationPin.addEventListener("click", () => {
  commitTextEditor();
  void pinSelection();
});

annotationTextEditor.addEventListener("pointerdown", (event) => event.stopPropagation());
annotationTextEditor.addEventListener("contextmenu", (event) => event.stopPropagation());

annotationTextConfirm.addEventListener("click", () => {
  commitTextEditor();
});

annotationTextCancel.addEventListener("click", () => {
  cancelTextEditor();
});

annotationTextInput.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    cancelTextEditor();
  } else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    event.stopPropagation();
    commitTextEditor();
  }
});

annotationCanvas.addEventListener("pointerdown", (event) => {
  if (event.button !== 0 || !selectedImage) return;
  event.stopPropagation();
  const point = annotationPointFromEvent(event);
  if (annotationTool === "text") {
    openTextEditor(point);
    return;
  }

  annotationStart = point;
  annotationPointerId = event.pointerId;
  annotationCanvas.setPointerCapture(event.pointerId);
  const width = currentAnnotationWidth();
  annotationDraft = annotationTool === "brush"
    ? { kind: "brush", points: [point], color: annotationColor.value, width }
    : annotationTool === "mosaic"
      ? { kind: "mosaic", rect: annotationRect(point, point), blockSize: Math.max(6, width * 2) }
      : annotationTool === "arrow"
        ? { kind: "arrow", start: point, end: point, color: annotationColor.value, width }
        : { kind: annotationTool, rect: annotationRect(point, point), color: annotationColor.value, width } as Annotation;
  scheduleAnnotationRender();
});

annotationCanvas.addEventListener("pointermove", (event) => {
  if (!annotationStart || event.pointerId !== annotationPointerId || !annotationDraft) return;
  event.stopPropagation();
  const point = annotationPointFromEvent(event);
  switch (annotationDraft.kind) {
    case "arrow": annotationDraft = { ...annotationDraft, end: point }; break;
    case "rectangle":
    case "ellipse":
    case "mosaic": annotationDraft = { ...annotationDraft, rect: annotationRect(annotationStart, point) }; break;
    case "brush": {
      if (annotationDraft.points.length >= MAX_BRUSH_POINTS) return;
      const previous = annotationDraft.points.at(-1);
      const minimumDistance = Math.max(1.5, annotationDraft.width * 0.25);
      if (previous && Math.hypot(point.x - previous.x, point.y - previous.y) < minimumDistance) {
        return;
      }
      annotationDraft = { ...annotationDraft, points: [...annotationDraft.points, point] };
      break;
    }
    default: break;
  }
  scheduleAnnotationRender();
});

annotationCanvas.addEventListener("pointerup", (event) => {
  if (event.pointerId !== annotationPointerId || !annotationDraft) return;
  event.stopPropagation();
  if (annotationCanvas.hasPointerCapture(event.pointerId)) annotationCanvas.releasePointerCapture(event.pointerId);
  const draft = annotationDraft.kind === "brush"
    ? {
        ...annotationDraft,
        points: simplifyPolyline(
          annotationDraft.points,
          Math.max(0.75, annotationDraft.width * 0.18),
        ),
      }
    : annotationDraft;
  annotationStart = null;
  annotationPointerId = null;
  const hasArea = draft.kind === "brush" || draft.kind === "arrow" || draft.kind === "text" || (draft.rect.width >= 1 && draft.rect.height >= 1);
  if (hasArea) commitAnnotation(draft);
  else {
    annotationDraft = null;
    scheduleAnnotationRender();
  }
});

annotationCanvas.addEventListener("pointercancel", (event) => {
  if (event.pointerId !== annotationPointerId) return;
  annotationStart = null;
  annotationDraft = null;
  annotationPointerId = null;
  scheduleAnnotationRender();
});

await listen("capture://reset", async () => {
  const totalStarted = performance.now();
  resetSelectionUi();
  const version = captureSessionVersion;
  overlay.dataset.state = "loading";
  captureStatus.textContent = "正在载入屏幕截图…";
  captureReady = false;
  currentCapture = null;
  monitorGuides.replaceChildren();
  captureDesktop.textContent = "正在读取虚拟桌面…";
  capturePointer.textContent = "坐标：等待截图";

  try {
    const metadataStarted = performance.now();
    const capture = await getCurrentCapture();
    const metadataIpcMs = performance.now() - metadataStarted;
    if (version !== captureSessionVersion) return;
    const pngStarted = performance.now();
    const png = await getCurrentCaptureImage();
    const pngIpcMs = performance.now() - pngStarted;
    if (version !== captureSessionVersion) return;

    const objectUrl = URL.createObjectURL(new Blob([png], { type: "image/png" }));
    captureImage.src = objectUrl;
    const decodeStarted = performance.now();
    try {
      await captureImage.decode();
    } catch (error) {
      URL.revokeObjectURL(objectUrl);
      throw error;
    }
    if (version !== captureSessionVersion) {
      URL.revokeObjectURL(objectUrl);
      return;
    }

    revokeObjectUrl(captureImageObjectUrl);
    captureImageObjectUrl = objectUrl;
    currentCapture = capture;
    renderMonitorGuides(capture);
    const decodeMs = performance.now() - decodeStarted;
    captureDesktop.textContent = `虚拟桌面：(${capture.desktop.x}, ${capture.desktop.y}) ${capture.width} × ${capture.height} · ${capture.desktop.monitors.length} 个显示器 · 性能：抓屏 ${formatMilliseconds(capture.captureMs)} · PNG/IPC ${formatMilliseconds(pngIpcMs)} · 解码 ${formatMilliseconds(decodeMs)}`;
    captureStatus.textContent = `${capture.width} × ${capture.height} · ${capture.desktop.monitors.length} 屏 · 拖动选择区域`;

    // Decode while hidden, then reveal a transparent WebView2 surface first.
    // Hidden WebViews can throttle animation frames, so the two-frame compositor
    // warm-up must happen after the native window becomes visible.
    await revealCaptureOverlay();
    if (version !== captureSessionVersion) return;
    await waitForCompositorWarmup();
    if (version !== captureSessionVersion) return;
    overlay.dataset.state = "selecting";
    captureReady = true;
    const totalMs = performance.now() - totalStarted;
    reportPerformance("capture", {
      rustCapture: capture.captureMs,
      metadataIpc: metadataIpcMs,
      pngIpc: pngIpcMs,
      browserDecode: decodeMs,
      overlayLoad: totalMs,
    });
  } catch (error) {
    if (version !== captureSessionVersion) return;
    captureReady = false;
    currentCapture = null;
    overlay.dataset.state = "idle";
    try {
      await cancelCapture();
    } catch (cleanupError) {
      console.error("failed to clear an incomplete screen capture", cleanupError);
    }
    captureStatus.textContent = "截图载入失败，请重新触发快捷键";
    console.error("failed to load current screen capture", error);
  }
});

window.addEventListener("beforeunload", releaseImageResources);
