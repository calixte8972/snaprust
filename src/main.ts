import { listen } from "@tauri-apps/api/event";

import {
  cancelCapture,
  cancelTranslation,
  captureScrollingSelection,
  copyHistoryCapture,
  copyText,
  copySelectedCapture,
  cropSelectedCapture,
  deleteHistoryCapture,
  deleteHistoryCaptures,
  exportHistoryCaptures,
  getCurrentCapture,
  getCurrentCaptureImage,
  getHistoryThumbnail,
  getHistoryUsage,
  getSelectedCaptureImage,
  getTranslationConfig,
  hideHistoryWindow,
  listHistory,
  listOcrLanguages,
  listTranslationModels,
  listTranslationProviders,
  pinHistoryCapture,
  pinSelectedCapture,
  recognizeSelectedCapture,
  revealCaptureOverlay,
  selectCaptureRegion,
  rotateSelectedCapture,
  saveTranslationConfig,
  setCaptureAnnotations,
  setCaptureFrame,
  setHistoryFavorite,
  setHistoryFavoriteBatch,
  setHistoryTags,
  translateText,
  type Annotation,
  type AnnotationPoint,
  type AnnotationRect,
  type CapturePayload,
  type FrameStyle,
  type HistoryItemPayload,
  type OcrLinePayload,
  type SelectionCropRect,
  type SelectionPayload,
  type TranslationProviderPayload,
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
const settingsPanel = requireElement("#settings-panel");
const settingsClose = requireElement<HTMLButtonElement>("#settings-close");
const translationSettingsForm = requireElement<HTMLFormElement>("#translation-settings-form");
const settingsProvider = requireElement<HTMLSelectElement>("#settings-provider");
const settingsApiKey = requireElement<HTMLInputElement>("#settings-api-key");
const settingsApiKeyLabel = requireElement("#settings-api-key-label");
const settingsApiKeyHint = requireElement("#settings-api-key-hint");
const settingsModel = requireElement<HTMLInputElement>("#settings-model");
const settingsEndpoint = requireElement<HTMLInputElement>("#settings-endpoint");
const settingsClearKey = requireElement<HTMLInputElement>("#settings-clear-key");
const settingsStatus = requireElement("#settings-status");
const settingsTest = requireElement<HTMLButtonElement>("#settings-test");
const settingsSave = requireElement<HTMLButtonElement>("#settings-save");
const selectionBox = requireElement("#selection-box");
const selectionSize = requireElement("#selection-size");
const annotationEditor = requireElement("#annotation-editor");
const historyPanel = requireElement("#history-panel");
const historyList = requireElement("#history-list");
const historySummary = requireElement("#history-summary");
const historyStorage = requireElement("#history-storage");
const historySearch = requireElement<HTMLInputElement>("#history-search");
const historyFavorites = requireElement<HTMLInputElement>("#history-favorites");
const historyClose = requireElement<HTMLButtonElement>("#history-close");
const historyBatch = requireElement("#history-batch");
const historySelectAll = requireElement<HTMLInputElement>("#history-select-all");
const historySelectedCount = requireElement("#history-selected-count");
const historyBatchFavorite = requireElement<HTMLButtonElement>("#history-batch-favorite");
const historyBatchUnfavorite = requireElement<HTMLButtonElement>("#history-batch-unfavorite");
const historyBatchExport = requireElement<HTMLButtonElement>("#history-batch-export");
const historyBatchDelete = requireElement<HTMLButtonElement>("#history-batch-delete");
const annotationCanvasWrap = requireElement("#annotation-canvas-wrap");
const annotationFrame = requireElement("#annotation-frame");
const annotationCanvas = requireElement<HTMLCanvasElement>("#annotation-canvas");
const annotationTextEditor = requireElement("#annotation-text-editor");
const annotationTextInput = requireElement<HTMLTextAreaElement>("#annotation-text-input");
const annotationTextConfirm = requireElement<HTMLButtonElement>("#annotation-text-confirm");
const annotationTextCancel = requireElement<HTMLButtonElement>("#annotation-text-cancel");
const annotationStatus = requireElement("#annotation-status");
const annotationColor = requireElement<HTMLInputElement>("#annotation-color");
const annotationWidth = requireElement<HTMLInputElement>("#annotation-width");
const annotationWidthValue = requireElement("#annotation-width-value");
const annotationZoom = requireElement<HTMLInputElement>("#annotation-zoom");
const annotationZoomValue = requireElement("#annotation-zoom-value");
const annotationZoomOut = requireElement<HTMLButtonElement>("#annotation-zoom-out");
const annotationZoomIn = requireElement<HTMLButtonElement>("#annotation-zoom-in");
const annotationZoomFit = requireElement<HTMLButtonElement>("#annotation-zoom-fit");
const annotationCropActions = requireElement("#annotation-crop-actions");
const annotationCropCancel = requireElement<HTMLButtonElement>("#annotation-crop-cancel");
const annotationCropApply = requireElement<HTMLButtonElement>("#annotation-crop-apply");
const annotationFrameSelect = requireElement<HTMLSelectElement>("#annotation-frame-select");
const annotationUndo = requireElement<HTMLButtonElement>("#annotation-undo");
const annotationRedo = requireElement<HTMLButtonElement>("#annotation-redo");
const annotationClear = requireElement<HTMLButtonElement>("#annotation-clear");
const annotationScrollCapture = requireElement<HTMLButtonElement>("#annotation-scroll-capture");
const annotationOcr = requireElement<HTMLButtonElement>("#annotation-ocr");
const annotationTranslate = requireElement<HTMLButtonElement>("#annotation-translate");
const annotationPin = requireElement<HTMLButtonElement>("#annotation-pin");
const ocrPanel = requireElement("#ocr-panel");
const ocrMeta = requireElement("#ocr-meta");
const ocrLanguage = requireElement<HTMLSelectElement>("#ocr-language");
const ocrResult = requireElement<HTMLTextAreaElement>("#ocr-result");
const ocrLines = requireElement<HTMLDivElement>("#ocr-lines");
const ocrCopy = requireElement<HTMLButtonElement>("#ocr-copy");
const ocrClose = requireElement<HTMLButtonElement>("#ocr-close");
const translationTarget = requireElement<HTMLSelectElement>("#translation-target");
const translationModel = requireElement<HTMLSelectElement>("#translation-model");
const translationRun = requireElement<HTMLButtonElement>("#translation-run");
const translationResult = requireElement<HTMLTextAreaElement>("#translation-result");
const translationMeta = requireElement("#translation-meta");
const translationCopy = requireElement<HTMLButtonElement>("#translation-copy");
const imageContextMenu = requireElement<HTMLElement>("#image-context-menu");
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
let ocrInProgress = false;
let translationInProgress = false;
let cropInProgress = false;
let frameInProgress = false;
let scrollCaptureInProgress = false;
let nextTranslationRequestId = 0;
let activeTranslationRequestId: number | null = null;
let ocrLanguagesLoaded = false;
let ocrLanguagesLoading = false;
let translationModelsLoaded = false;
let translationModelsLoading = false;
let translationProviders: ReadonlyArray<TranslationProviderPayload> = [];
let translationProvidersLoading = false;
let settingsInProgress = false;
let settingsRequestVersion = 0;
let rotationQuarters = 0;
let currentCapture: CapturePayload | null = null;
let annotationFrameStyle: FrameStyle = "none";
type AnnotationTool = "arrow" | "rectangle" | "ellipse" | "brush" | "mosaic" | "text" | "crop";
let annotationTool: AnnotationTool = "arrow";
let selectedImage: HTMLImageElement | null = null;
let annotations: Annotation[] = [];
let annotationRedoStack: Annotation[][] = [];
let annotationStart: AnnotationPoint | null = null;
let annotationDraft: Annotation | null = null;
let cropStart: AnnotationPoint | null = null;
let cropDraft: AnnotationRect | null = null;
let annotationPointerId: number | null = null;
let annotationRenderFrame: number | null = null;
let annotationZoomFactor = 1;
let annotationZoomBaseScale = 1;
let captureImageObjectUrl: string | null = null;
let selectedImageObjectUrl: string | null = null;
let captureSessionVersion = 0;
const MAX_BRUSH_POINTS = 20_000;
let pendingTextAnnotation: Readonly<{
  position: AnnotationPoint;
  color: string;
  fontSize: number;
}> | null = null;
let highlightedOcrLine: OcrLinePayload | null = null;
let historyLoadVersion = 0;
let historySearchTimer: number | null = null;
const historyThumbnailUrls = new Map<number, string>();
let historyThumbnailObserver: IntersectionObserver | null = null;
let historyVisibleIds: number[] = [];
const selectedHistoryIds = new Set<number>();

function formatMilliseconds(value: number): string {
  return `${value.toFixed(value < 10 ? 1 : 0)}ms`;
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let formatted = value / 1024;
  let unitIndex = 0;
  while (formatted >= 1024 && unitIndex < units.length - 1) {
    formatted /= 1024;
    unitIndex += 1;
  }
  return `${formatted.toFixed(formatted < 10 ? 1 : 0)} ${units[unitIndex]}`;
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

function releaseHistoryThumbnails(): void {
  historyThumbnailObserver?.disconnect();
  historyThumbnailObserver = null;
  historyThumbnailUrls.forEach((url) => URL.revokeObjectURL(url));
  historyThumbnailUrls.clear();
}

function observeHistoryThumbnail(image: HTMLImageElement, id: number): void {
  image.dataset.historyId = String(id);
  if (typeof IntersectionObserver === "undefined") {
    void loadHistoryThumbnail(id, image, historyLoadVersion);
    return;
  }

  historyThumbnailObserver ??= new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        const target = entry.target as HTMLImageElement;
        const targetId = Number(target.dataset.historyId);
        historyThumbnailObserver?.unobserve(target);
        if (Number.isInteger(targetId)) {
          void loadHistoryThumbnail(targetId, target, historyLoadVersion);
        }
      }
    },
    { root: historyList, rootMargin: "280px 0px" },
  );
  historyThumbnailObserver.observe(image);
}

function cancelActiveTranslationRequest(): void {
  const requestId = activeTranslationRequestId;
  activeTranslationRequestId = null;
  if (requestId !== null) {
    void cancelTranslation(requestId).catch((error) => {
      console.debug("failed to cancel translation request", error);
    });
  }
}

function resetSelectionUi(): void {
  captureSessionVersion += 1;
  settingsRequestVersion += 1;
  cancelActiveTranslationRequest();
  settingsInProgress = false;
  settingsSave.disabled = false;
  settingsTest.disabled = false;
  settingsPanel.hidden = true;
  releaseImageResources();
  releaseHistoryThumbnails();
  dragStart = null;
  activePointerId = null;
  rotationQuarters = 0;
  annotationCanvas.style.transform = "";
  currentSelection = null;
  selectedCapture = null;
  copyInProgress = false;
  pinInProgress = false;
  ocrInProgress = false;
  translationInProgress = false;
  cropInProgress = false;
  frameInProgress = false;
  scrollCaptureInProgress = false;
  annotationCanvas.style.width = "";
  annotationCanvas.style.height = "";
  annotationCanvas.style.maxWidth = "";
  annotationCanvas.style.maxHeight = "";
  annotationZoomFactor = 1;
  annotationZoomBaseScale = 1;
  annotationZoom.value = "100";
  annotationZoomValue.textContent = "100%";
  overlay.dataset.hasSelection = "false";
  selectionBox.style.cssText = "";
  selectionSize.textContent = "";
  annotationEditor.hidden = true;
  annotationEditor.style.removeProperty("--annotation-editor-width");
  annotationEditor.style.removeProperty("--annotation-editor-height");
  annotationFrameStyle = "none";
  updateAnnotationFramePreview();
  historyPanel.hidden = true;
  overlay.dataset.state = "idle";
  annotations = [];
  annotationRedoStack = [];
  annotationStart = null;
  annotationDraft = null;
  cropStart = null;
  cropDraft = null;
  annotationEditor.dataset.mode = "";
  annotationCropActions.hidden = true;
  annotationCropApply.disabled = true;
  annotationPointerId = null;
  annotationUndo.disabled = true;
  annotationRedo.disabled = true;
  annotationClear.disabled = true;
  annotationFrameSelect.disabled = false;
  annotationScrollCapture.disabled = false;
  annotationPin.disabled = false;
  annotationOcr.disabled = false;
  ocrPanel.hidden = true;
  ocrMeta.textContent = "等待识别";
  ocrResult.value = "";
  renderOcrLines([]);
  ocrCopy.disabled = true;
  ocrCopy.textContent = "复制文字";
  translationResult.value = "";
  translationMeta.textContent = "需要配置翻译服务";
  translationCopy.disabled = true;
  translationCopy.textContent = "复制译文";
  translationRun.disabled = false;
  annotationTranslate.disabled = false;
  ocrLanguage.disabled = !ocrLanguagesLoaded || ocrLanguage.options.length <= 1;
  cancelTextEditor();
}

function formatHistoryDate(createdAtMs: number): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(createdAtMs));
}

function historyOcrPreview(item: HistoryItemPayload): string {
  const text = item.ocrText?.replace(/\s+/g, " ").trim();
  return text || "未保存 OCR 文字";
}

function parseHistoryTags(value: string): string[] {
  return value
    .split(/[,，]/)
    .map((tag) => tag.trim())
    .filter((tag) => tag.length > 0);
}

function makeHistoryAction(label: string, title: string): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "history-card__action";
  button.textContent = label;
  button.title = title;
  return button;
}

function updateHistoryBatchControls(): void {
  const selectedCount = selectedHistoryIds.size;
  const visibleCount = historyVisibleIds.length;
  historyBatch.hidden = visibleCount === 0;
  historySelectedCount.textContent = `已选择 ${selectedCount} 项`;
  historySelectAll.checked = visibleCount > 0 && selectedCount === visibleCount;
  historySelectAll.indeterminate = selectedCount > 0 && selectedCount < visibleCount;
  historyBatchFavorite.disabled = selectedCount === 0;
  historyBatchUnfavorite.disabled = selectedCount === 0;
  historyBatchExport.disabled = selectedCount === 0;
  historyBatchDelete.disabled = selectedCount === 0;
}

function currentSelectedHistoryIds(): number[] {
  return historyVisibleIds.filter((id) => selectedHistoryIds.has(id));
}

function renderHistory(items: ReadonlyArray<HistoryItemPayload>): void {
  releaseHistoryThumbnails();
  historyList.replaceChildren();
  historyVisibleIds = items.map((item) => item.id);
  selectedHistoryIds.clear();
  updateHistoryBatchControls();
  historySummary.textContent = items.length === 0
    ? "没有符合条件的截图"
    : `显示最近 ${items.length} 条本地截图`;
  if (items.length === 0) {
    const empty = document.createElement("p");
    empty.className = "history-empty";
    empty.textContent = "完成一次复制图片或钉图后，成品截图会自动出现在这里。";
    historyList.append(empty);
    return;
  }

  items.forEach((item) => {
    const card = document.createElement("article");
    card.className = "history-card";
    const thumbnail = document.createElement("img");
    thumbnail.className = "history-card__thumbnail";
    thumbnail.loading = "lazy";
    thumbnail.decoding = "async";
    thumbnail.alt = `${formatHistoryDate(item.createdAtMs)} 的截图缩略图`;
    const thumbnailWrap = document.createElement("div");
    thumbnailWrap.className = "history-card__thumbnail-wrap";
    const selectLabel = document.createElement("label");
    selectLabel.className = "history-card__select";
    const select = document.createElement("input");
    select.type = "checkbox";
    select.dataset.historyId = String(item.id);
    select.checked = selectedHistoryIds.has(item.id);
    select.setAttribute("aria-label", `选择 ${formatHistoryDate(item.createdAtMs)} 的截图`);
    select.addEventListener("change", () => {
      if (select.checked) selectedHistoryIds.add(item.id);
      else selectedHistoryIds.delete(item.id);
      updateHistoryBatchControls();
    });
    selectLabel.append(select);
    thumbnailWrap.append(thumbnail);
    thumbnailWrap.append(selectLabel);
    const content = document.createElement("div");
    content.className = "history-card__content";
    const meta = document.createElement("span");
    meta.className = "history-card__meta";
    meta.textContent = `${formatHistoryDate(item.createdAtMs)} · ${item.width} × ${item.height}`;
    const preview = document.createElement("p");
    preview.className = "history-card__preview";
    preview.textContent = historyOcrPreview(item);
    const tags = document.createElement("div");
    tags.className = "history-card__tags";
    item.tags.forEach((tag) => {
      const chip = document.createElement("span");
      chip.className = "history-tag";
      chip.textContent = tag;
      tags.append(chip);
    });
    const tagEditor = document.createElement("form");
    tagEditor.className = "history-card__tag-editor";
    tagEditor.hidden = true;
    const tagInput = document.createElement("input");
    tagInput.type = "text";
    tagInput.maxLength = 512;
    tagInput.value = item.tags.join(", ");
    tagInput.placeholder = "标签，以逗号分隔";
    tagInput.setAttribute("aria-label", "历史标签，以逗号分隔");
    const tagSave = makeHistoryAction("保存", "保存标签");
    tagSave.type = "submit";
    tagEditor.append(tagInput, tagSave);
    const actions = document.createElement("div");
    actions.className = "history-card__actions";
    const copy = makeHistoryAction("复制", "复制图片到剪贴板");
    const pin = makeHistoryAction("📌", "重新钉图");
    const favorite = makeHistoryAction(item.favorite ? "★" : "☆", item.favorite ? "取消收藏" : "收藏");
    favorite.classList.toggle("is-favorite", item.favorite);
    const editTags = makeHistoryAction("标签", "编辑标签");
    const remove = makeHistoryAction("删除", "永久删除这条历史记录和图片");
    remove.classList.add("history-card__action--danger");
    copy.addEventListener("click", () => void copyHistory(item.id, copy));
    pin.addEventListener("click", () => void pinHistory(item.id, pin));
    favorite.addEventListener("click", () => void toggleHistoryFavorite(item, favorite));
    editTags.addEventListener("click", () => {
      tagEditor.hidden = false;
      tagInput.focus();
      tagInput.select();
    });
    tagEditor.addEventListener("submit", (event) => {
      event.preventDefault();
      void saveHistoryTags(item.id, parseHistoryTags(tagInput.value), tagSave);
    });
    remove.addEventListener("click", () => void removeHistory(item));
    actions.append(copy, pin, favorite, editTags, remove);
    content.append(meta, preview, tags, tagEditor, actions);
    card.append(thumbnailWrap, content);
    historyList.append(card);
    observeHistoryThumbnail(thumbnail, item.id);
  });
}

async function loadHistoryThumbnail(id: number, image: HTMLImageElement, version: number): Promise<void> {
  try {
    const png = await getHistoryThumbnail(id);
    if (version !== historyLoadVersion || historyPanel.hidden || !historyList.contains(image)) return;
    const objectUrl = URL.createObjectURL(new Blob([png], { type: "image/png" }));
    historyThumbnailUrls.set(id, objectUrl);
    image.src = objectUrl;
  } catch (error) {
    image.alt = "缩略图加载失败";
    console.error("failed to load history thumbnail", error);
  }
}

async function loadHistory(): Promise<void> {
  const version = ++historyLoadVersion;
  releaseHistoryThumbnails();
  historyList.replaceChildren();
  historySummary.textContent = "正在读取本地截图…";
  try {
    const query = historySearch.value.trim() || undefined;
    const [items, usage] = await Promise.all([
      listHistory(query, historyFavorites.checked),
      getHistoryUsage(),
    ]);
    if (version !== historyLoadVersion || historyPanel.hidden) return;
    historyStorage.textContent = `已用 ${formatBytes(usage.imageBytes)} / ${formatBytes(usage.maxImageBytes)} · ${usage.itemCount}/${usage.maxItems} 条`;
    historyStorage.title = "达到任一上限后，SnapRust 会自动清理最旧的未收藏截图；收藏截图不会被自动删除。";
    renderHistory(items);
  } catch (error) {
    if (version !== historyLoadVersion || historyPanel.hidden) return;
    historySummary.textContent = "历史读取失败";
    const message = document.createElement("p");
    message.className = "history-empty";
    message.textContent = `无法读取截图历史：${String(error)}`;
    historyList.append(message);
    console.error("failed to list screenshot history", error);
  }
}

async function copyHistory(id: number, button: HTMLButtonElement): Promise<void> {
  button.disabled = true;
  const previous = button.textContent;
  try {
    await copyHistoryCapture(id);
    button.textContent = "已复制";
  } catch (error) {
    button.textContent = "失败";
    console.error("failed to copy history capture", error);
  } finally {
    window.setTimeout(() => {
      button.textContent = previous;
      button.disabled = false;
    }, 1_100);
  }
}

async function pinHistory(id: number, button: HTMLButtonElement): Promise<void> {
  button.disabled = true;
  const previous = button.textContent;
  try {
    await pinHistoryCapture(id);
    button.textContent = "已钉";
  } catch (error) {
    button.textContent = "失败";
    console.error("failed to pin history capture", error);
  } finally {
    window.setTimeout(() => {
      button.textContent = previous;
      button.disabled = false;
    }, 1_100);
  }
}

async function toggleHistoryFavorite(item: HistoryItemPayload, button: HTMLButtonElement): Promise<void> {
  button.disabled = true;
  try {
    await setHistoryFavorite(item.id, !item.favorite);
    void loadHistory();
  } catch (error) {
    button.disabled = false;
    console.error("failed to update history favorite", error);
  }
}

async function saveHistoryTags(id: number, tags: ReadonlyArray<string>, button: HTMLButtonElement): Promise<void> {
  button.disabled = true;
  try {
    await setHistoryTags(id, tags);
    void loadHistory();
  } catch (error) {
    button.disabled = false;
    console.error("failed to update history tags", error);
  }
}

async function removeHistory(item: HistoryItemPayload): Promise<void> {
  if (!window.confirm(`永久删除 ${formatHistoryDate(item.createdAtMs)} 的截图吗？此操作不可恢复。`)) return;
  try {
    await deleteHistoryCapture(item.id);
    void loadHistory();
  } catch (error) {
    console.error("failed to remove history capture", error);
  }
}

async function setHistoryFavoritesInBatch(favorite: boolean): Promise<void> {
  const ids = currentSelectedHistoryIds();
  if (ids.length === 0) return;
  historyBatchFavorite.disabled = true;
  historyBatchUnfavorite.disabled = true;
  historyBatchExport.disabled = true;
  historyBatchDelete.disabled = true;
  try {
    await setHistoryFavoriteBatch(ids, favorite);
    void loadHistory();
  } catch (error) {
    updateHistoryBatchControls();
    console.error("failed to update history favorites in batch", error);
  }
}

async function exportHistoryInBatch(): Promise<void> {
  const ids = currentSelectedHistoryIds();
  if (ids.length === 0) return;
  historyBatchFavorite.disabled = true;
  historyBatchUnfavorite.disabled = true;
  historyBatchExport.disabled = true;
  historyBatchDelete.disabled = true;
  const previous = historyBatchExport.textContent;
  historyBatchExport.textContent = "导出中…";
  try {
    const result = await exportHistoryCaptures(ids);
    historyStorage.textContent = `已导出 ${result.exportedCount} 张到：${result.directory}`;
    historyStorage.title = result.directory;
  } catch (error) {
    historyStorage.textContent = `历史导出失败：${String(error)}`;
    historyStorage.title = "历史导出失败";
    console.error("failed to export history captures", error);
  } finally {
    historyBatchExport.textContent = previous;
    updateHistoryBatchControls();
  }
}

async function removeHistoryInBatch(): Promise<void> {
  const ids = currentSelectedHistoryIds();
  if (ids.length === 0) return;
  if (!window.confirm(`永久删除选中的 ${ids.length} 张截图吗？此操作不可恢复。`)) return;
  historyBatchFavorite.disabled = true;
  historyBatchUnfavorite.disabled = true;
  historyBatchExport.disabled = true;
  historyBatchDelete.disabled = true;
  try {
    await deleteHistoryCaptures(ids);
    selectedHistoryIds.clear();
    void loadHistory();
  } catch (error) {
    updateHistoryBatchControls();
    console.error("failed to remove history captures in batch", error);
  }
}

async function closeHistory(): Promise<void> {
  if (historyPanel.hidden) return;
  historyLoadVersion += 1;
  releaseHistoryThumbnails();
  historyPanel.hidden = true;
  overlay.dataset.state = "idle";
  try {
    await hideHistoryWindow();
  } catch (error) {
    console.error("failed to hide history window", error);
  }
}

function currentAnnotationWidth(): number {
  return Number(annotationWidth.value);
}

type FrameMetrics = Readonly<{
  side: number;
  top: number;
  bottom: number;
}>;

type MacosFrameMetrics = FrameMetrics & Readonly<{
  dotRadius: number;
  dotFirstX: number;
  dotStep: number;
}>;

function frameMetricScaler(imageWidth: number, imageHeight: number): (value: number) => number {
  const area = Math.max(1, imageWidth) * Math.max(1, imageHeight);
  const scale = Math.max(0.75, Math.min(2, Math.sqrt(area / (1280 * 720))));
  return (value: number): number => Math.max(1, Math.round(value * scale));
}

function macosFrameMetrics(imageWidth: number, imageHeight: number): MacosFrameMetrics {
  const scaled = frameMetricScaler(imageWidth, imageHeight);
  return {
    side: scaled(12),
    top: scaled(38),
    bottom: scaled(12),
    dotRadius: scaled(6),
    dotFirstX: scaled(20),
    dotStep: scaled(16),
  };
}

function annotationFrameMetrics(
  style: FrameStyle,
  imageWidth: number,
  imageHeight: number,
): FrameMetrics {
  const scaled = frameMetricScaler(imageWidth, imageHeight);
  switch (style) {
    case "macos":
      return macosFrameMetrics(imageWidth, imageHeight);
    case "windows11":
      return { side: scaled(8), top: scaled(40), bottom: scaled(8) };
    case "polaroid":
      return { side: scaled(24), top: scaled(24), bottom: scaled(72) };
    case "none":
      return { side: 0, top: 0, bottom: 0 };
  }
}

function annotationFrameInsets(
  imageWidth = annotationCanvas.width,
  imageHeight = annotationCanvas.height,
): { width: number; height: number } {
  const metrics = annotationFrameMetrics(annotationFrameStyle, imageWidth, imageHeight);
  return {
    width: metrics.side * 2,
    height: metrics.top + metrics.bottom,
  };
}

function updateAnnotationFrameDisplayScale(displayScale: number): void {
  const metrics = annotationFrameMetrics(
    annotationFrameStyle,
    annotationCanvas.width,
    annotationCanvas.height,
  );
  const macosMetrics = macosFrameMetrics(annotationCanvas.width, annotationCanvas.height);
  const scaled = frameMetricScaler(annotationCanvas.width, annotationCanvas.height);
  const px = (value: number): string => `${Math.max(1, value * displayScale)}px`;
  annotationFrame.style.setProperty("--annotation-frame-side", px(metrics.side));
  annotationFrame.style.setProperty("--annotation-frame-top", px(metrics.top));
  annotationFrame.style.setProperty("--annotation-frame-bottom", px(metrics.bottom));
  annotationFrame.style.setProperty("--annotation-frame-dot-size", px(macosMetrics.dotRadius * 2));
  annotationFrame.style.setProperty(
    "--annotation-frame-dot-left",
    px(macosMetrics.dotFirstX - macosMetrics.dotRadius),
  );
  annotationFrame.style.setProperty(
    "--annotation-frame-dot-gap",
    px(Math.max(1, macosMetrics.dotStep - macosMetrics.dotRadius * 2)),
  );
  annotationFrame.style.setProperty("--annotation-frame-windows-icon", px(scaled(12)));
  annotationFrame.style.setProperty("--annotation-frame-windows-icon-left", px(scaled(14)));
  annotationFrame.style.setProperty("--annotation-frame-windows-control", px(scaled(46)));
  annotationFrame.style.setProperty("--annotation-frame-windows-stroke", px(scaled(1)));
}

function updateAnnotationFramePreview(): void {
  annotationFrame.dataset.style = annotationFrameStyle;
  annotationFrameSelect.value = annotationFrameStyle;
  updateAnnotationFrameDisplayScale(
    Math.max(0.05, annotationZoomBaseScale * annotationZoomFactor),
  );
}

async function setAnnotationFrameStyle(style: FrameStyle): Promise<void> {
  if (
    !selectedCapture
    || cropInProgress
    || frameInProgress
    || ocrInProgress
    || translationInProgress
    || copyInProgress
    || pinInProgress
    || scrollCaptureInProgress
    || annotationTool === "crop"
    || style === annotationFrameStyle
  ) return;

  frameInProgress = true;
  annotationFrameSelect.disabled = true;
  const version = captureSessionVersion;
  try {
    await setCaptureFrame(style);
    if (version !== captureSessionVersion || annotationEditor.hidden) return;
    annotationFrameStyle = style;
    updateAnnotationFramePreview();
    updateAnnotationEditorLayout();
    scheduleAnnotationCanvasFit();
    const label = annotationFrameSelect.selectedOptions[0]?.textContent ?? style;
    annotationStatus.textContent = style === "none"
      ? "已移除图片边框"
      : `已应用${label}边框；复制或钉图时会一起输出`;
  } catch (error) {
    annotationFrameSelect.value = annotationFrameStyle;
    annotationStatus.textContent = `边框设置失败：${String(error)}`;
  } finally {
    if (version === captureSessionVersion) {
      frameInProgress = false;
      annotationFrameSelect.disabled = false;
    }
  }
}

function setAnnotationTool(tool: AnnotationTool): void {
  if (tool !== "text") cancelTextEditor();
  if (tool !== "crop") {
    cropStart = null;
    cropDraft = null;
  }
  annotationTool = tool;
  document.querySelectorAll<HTMLButtonElement>("[data-tool]").forEach((button) => {
    const active = button.dataset.tool === tool;
    button.classList.toggle("is-active", active);
    button.setAttribute("aria-pressed", String(active));
  });
  annotationCanvas.style.cursor = tool === "text" ? "text" : "crosshair";
  annotationEditor.dataset.mode = tool === "crop" ? "cropping" : "";
  annotationCropActions.hidden = tool !== "crop";
  annotationCropApply.disabled = tool !== "crop" || cropDraft === null || cropInProgress;
  if (selectedImage) scheduleAnnotationRender();
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
  const scale = Math.max(0.0001, annotationCanvas.clientWidth / Math.max(1, annotationCanvas.width));
  const dx = (event.clientX - (bounds.left + bounds.width / 2)) / scale;
  const dy = (event.clientY - (bounds.top + bounds.height / 2)) / scale;
  let localX = dx;
  let localY = dy;
  switch (rotationQuarters) {
    case 1:
      localX = dy;
      localY = -dx;
      break;
    case 2:
      localX = -dx;
      localY = -dy;
      break;
    case 3:
      localX = -dy;
      localY = dx;
      break;
    default:
      break;
  }
  return {
    x: Math.max(0, Math.min(annotationCanvas.width, localX + annotationCanvas.width / 2)),
    y: Math.max(0, Math.min(annotationCanvas.height, localY + annotationCanvas.height / 2)),
  };
}

function annotationRect(start: AnnotationPoint, end: AnnotationPoint): AnnotationRect {
  return {
    x: Math.min(start.x, end.x),
    y: Math.min(start.y, end.y),
    width: Math.abs(end.x - start.x),
    height: Math.abs(end.y - start.y),
  };
}

function drawCropOverlay(
  context: CanvasRenderingContext2D,
  rect: AnnotationRect,
): void {
  const right = Math.min(context.canvas.width, rect.x + rect.width);
  const bottom = Math.min(context.canvas.height, rect.y + rect.height);
  context.save();
  context.fillStyle = "rgb(7 10 14 / 58%)";
  context.fillRect(0, 0, context.canvas.width, Math.max(0, rect.y));
  context.fillRect(0, bottom, context.canvas.width, Math.max(0, context.canvas.height - bottom));
  context.fillRect(0, rect.y, Math.max(0, rect.x), Math.max(0, rect.height));
  context.fillRect(right, rect.y, Math.max(0, context.canvas.width - right), Math.max(0, rect.height));
  context.strokeStyle = "#8af0c9";
  context.lineWidth = Math.max(2, Math.min(context.canvas.width, context.canvas.height) * 0.004);
  context.setLineDash([8, 6]);
  context.strokeRect(rect.x, rect.y, rect.width, rect.height);
  context.setLineDash([]);

  const label = `${Math.round(rect.width)} × ${Math.round(rect.height)}`;
  context.font = '12px ui-monospace, SFMono-Regular, Consolas, monospace';
  const labelWidth = context.measureText(label).width + 14;
  const labelX = Math.max(4, Math.min(context.canvas.width - labelWidth - 4, rect.x));
  const labelAbove = rect.y >= 26;
  const labelY = labelAbove ? rect.y - 8 : Math.min(context.canvas.height - 4, bottom + 20);
  context.fillStyle = "rgb(7 10 14 / 92%)";
  context.fillRect(labelX, labelY - 15, labelWidth, 19);
  context.fillStyle = "#f7f7f5";
  context.textBaseline = "middle";
  context.fillText(label, labelX + 7, labelY - 5);
  context.restore();
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
  annotationStatus.textContent = `${annotationCanvas.width} × ${annotationCanvas.height} · ${annotations.length} 个标注`;
  annotationUndo.disabled = annotations.length === 0;
  annotationRedo.disabled = annotationRedoStack.length === 0;
  annotationClear.disabled = annotations.length === 0;
}

function updateAnnotationEditorLayout(imageWidth = annotationCanvas.width, imageHeight = annotationCanvas.height): void {
  const frame = annotationFrameInsets(imageWidth, imageHeight);
  const availableWidth = Math.max(320, window.innerWidth - 32);
  const ocrWidth = ocrPanel.hidden
    ? 0
    : Math.min(380, Math.max(280, Math.round(window.innerWidth * 0.32)));
  const editorWidth = Math.min(
    1180,
    availableWidth,
    Math.max(640, imageWidth + 36 + frame.width + ocrWidth),
  );
  const editorHeight = Math.min(
    Math.max(ocrPanel.hidden ? 260 : 480, imageHeight + 130 + frame.height),
    Math.max(240, window.innerHeight - 32),
  );
  annotationEditor.style.setProperty("--annotation-editor-width", `${editorWidth}px`);
  annotationEditor.style.setProperty("--annotation-editor-height", `${editorHeight}px`);
}

function applyAnnotationZoom(): void {
  if (!selectedImage || annotationCanvas.width <= 0 || annotationCanvas.height <= 0) return;
  const scale = Math.max(0.05, annotationZoomBaseScale * annotationZoomFactor);
  annotationCanvas.style.width = `${Math.max(1, Math.round(annotationCanvas.width * scale))}px`;
  annotationCanvas.style.height = `${Math.max(1, Math.round(annotationCanvas.height * scale))}px`;
  annotationCanvas.style.maxWidth = "none";
  annotationCanvas.style.maxHeight = "none";
  updateAnnotationFrameDisplayScale(scale);
}

function fitAnnotationCanvas(): void {
  if (!selectedImage || annotationCanvas.width <= 0 || annotationCanvas.height <= 0) return;
  const frame = annotationFrameInsets();
  const availableWidth = Math.max(1, annotationCanvasWrap.clientWidth - 36);
  const availableHeight = Math.max(1, annotationCanvasWrap.clientHeight - 36);
  annotationZoomBaseScale = Math.min(
    1,
    availableWidth / (annotationCanvas.width + frame.width),
    availableHeight / (annotationCanvas.height + frame.height),
  );
  if (!Number.isFinite(annotationZoomBaseScale) || annotationZoomBaseScale <= 0) {
    annotationZoomBaseScale = 1;
  }
  applyAnnotationZoom();
}

function setAnnotationZoom(percent: number): void {
  const value = Math.max(50, Math.min(300, Math.round(percent / 10) * 10));
  annotationZoomFactor = value / 100;
  annotationZoom.value = String(value);
  annotationZoomValue.textContent = `${value}%`;
  applyAnnotationZoom();
}

function scheduleAnnotationCanvasFit(): void {
  window.requestAnimationFrame(() => {
    if (!annotationEditor.hidden) fitAnnotationCanvas();
  });
}

function drawOcrHighlight(line: OcrLinePayload, context: CanvasRenderingContext2D): void {
  const { x, y, width, height } = line.rect;
  if (width <= 0 || height <= 0) return;

  const padding = Math.max(2, Math.min(context.canvas.width, context.canvas.height) * 0.003);
  context.save();
  context.fillStyle = "rgb(67 217 163 / 20%)";
  context.strokeStyle = "rgb(138 240 201 / 94%)";
  context.lineWidth = Math.max(1, padding * 0.55);
  context.setLineDash([Math.max(3, padding * 1.8), Math.max(2, padding)]);
  context.fillRect(x - padding, y - padding, width + padding * 2, height + padding * 2);
  context.strokeRect(x - padding / 2, y - padding / 2, width + padding, height + padding);
  context.restore();
}

function renderAnnotationCanvas(): void {
  annotationRenderFrame = null;
  if (!selectedImage) return;
  annotationContext.clearRect(0, 0, annotationCanvas.width, annotationCanvas.height);
  annotationContext.drawImage(committedAnnotationCanvas, 0, 0);
  if (highlightedOcrLine) drawOcrHighlight(highlightedOcrLine, annotationContext);
  if (annotationDraft) drawAnnotation(annotationDraft, annotationContext);
  if (annotationTool === "crop" && cropDraft) drawCropOverlay(annotationContext, cropDraft);
}

function setHighlightedOcrLine(line: OcrLinePayload | null): void {
  if (highlightedOcrLine === line) return;
  highlightedOcrLine = line;
  scheduleAnnotationRender();
}

function scrollOcrLineIntoView(line: OcrLinePayload): void {
  const canvasBounds = annotationCanvas.getBoundingClientRect();
  const wrapBounds = annotationCanvasWrap.getBoundingClientRect();
  if (canvasBounds.width <= 0 || canvasBounds.height <= 0) return;
  const scale = canvasBounds.width / annotationCanvas.width;
  const targetLeft =
    canvasBounds.left - wrapBounds.left + annotationCanvasWrap.scrollLeft
    + (line.rect.x + line.rect.width / 2) * scale;
  const targetTop =
    canvasBounds.top - wrapBounds.top + annotationCanvasWrap.scrollTop
    + (line.rect.y + line.rect.height / 2) * scale;
  annotationCanvasWrap.scrollTo({
    left: Math.max(0, targetLeft - annotationCanvasWrap.clientWidth / 2),
    top: Math.max(0, targetTop - annotationCanvasWrap.clientHeight / 2),
    behavior: "smooth",
  });
}

function renderOcrLines(lines: ReadonlyArray<OcrLinePayload>): void {
  ocrLines.replaceChildren();
  ocrLines.hidden = lines.length === 0;
  if (lines.length === 0) {
    highlightedOcrLine = null;
    return;
  }

  lines.forEach((line, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ocr-line";
    button.title = `定位第 ${index + 1} 行：${line.text}`;
    button.setAttribute("aria-label", `定位第 ${index + 1} 行：${line.text || "未命名文字"}`);
    const number = document.createElement("span");
    number.className = "ocr-line__number";
    number.textContent = String(index + 1);
    const text = document.createElement("span");
    text.className = "ocr-line__text";
    text.textContent = line.text || "（未返回文字）";
    button.append(number, text);
    button.addEventListener("pointerenter", () => setHighlightedOcrLine(line));
    button.addEventListener("pointerleave", () => setHighlightedOcrLine(null));
    button.addEventListener("focus", () => setHighlightedOcrLine(line));
    button.addEventListener("blur", () => setHighlightedOcrLine(null));
    button.addEventListener("click", () => {
      setHighlightedOcrLine(line);
      scrollOcrLineIntoView(line);
    });
    ocrLines.append(button);
  });
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
  rotationQuarters = 0;
  annotationCanvas.style.transform = "";
  annotationFrameStyle = "none";
  updateAnnotationFramePreview();
  annotations = [];
  annotationRedoStack = [];
  updateAnnotationEditorLayout(selected.width, selected.height);
  annotationEditor.hidden = false;
  overlay.dataset.state = "editing";
  rebuildCommittedAnnotationCanvas();
  scheduleAnnotationCanvasFit();
  void loadAvailableOcrLanguages();
  void loadAvailableTranslationModels();
  return {
    pngIpcMs,
    decodeMs: performance.now() - decodeStarted,
    totalMs: performance.now() - totalStarted,
  };
}

function clearOcrResultsAfterCrop(): void {
  ocrPanel.hidden = true;
  ocrMeta.textContent = "等待识别";
  ocrResult.value = "";
  renderOcrLines([]);
  ocrCopy.disabled = true;
  ocrCopy.textContent = "复制文字";
  translationResult.value = "";
  translationMeta.textContent = "需要配置翻译服务";
  translationCopy.disabled = true;
  translationCopy.textContent = "复制译文";
  translationRun.disabled = false;
  ocrLanguage.disabled = !ocrLanguagesLoaded || ocrLanguage.options.length <= 1;
}

function cropRectForBackend(rect: AnnotationRect): SelectionCropRect {
  const left = Math.max(0, Math.min(annotationCanvas.width, Math.floor(rect.x)));
  const top = Math.max(0, Math.min(annotationCanvas.height, Math.floor(rect.y)));
  const right = Math.max(left, Math.min(annotationCanvas.width, Math.ceil(rect.x + rect.width)));
  const bottom = Math.max(top, Math.min(annotationCanvas.height, Math.ceil(rect.y + rect.height)));
  return {
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
  };
}

async function applyCropSelection(): Promise<void> {
  if (
    !selectedCapture
    || !cropDraft
    || cropInProgress
    || frameInProgress
    || ocrInProgress
    || translationInProgress
    || copyInProgress
    || pinInProgress
    || scrollCaptureInProgress
  ) return;

  const crop = cropRectForBackend(cropDraft);
  if (crop.width === 0 || crop.height === 0) return;
  if (annotations.length > 0 && !window.confirm("应用裁剪会清除当前标注，是否继续？")) return;

  cropInProgress = true;
  annotationCropApply.disabled = true;
  annotationCropCancel.disabled = true;
  annotationOcr.disabled = true;
  annotationTranslate.disabled = true;
  annotationPin.disabled = true;
  annotationScrollCapture.disabled = true;
  annotationStatus.textContent = "正在应用裁剪…";
  const version = captureSessionVersion;

  try {
    const cropped = await cropSelectedCapture(crop);
    if (version !== captureSessionVersion) return;
    const png = await getSelectedCaptureImage();
    const objectUrl = URL.createObjectURL(new Blob([png], { type: "image/png" }));
    const image = new Image();
    image.src = objectUrl;
    try {
      await image.decode();
    } catch (error) {
      URL.revokeObjectURL(objectUrl);
      throw error;
    }
    if (version !== captureSessionVersion) {
      URL.revokeObjectURL(objectUrl);
      return;
    }

    revokeObjectUrl(selectedImageObjectUrl);
    selectedImageObjectUrl = objectUrl;
    selectedImage = image;
    selectedCapture = cropped;
    annotationCanvas.width = cropped.width;
    annotationCanvas.height = cropped.height;
    committedAnnotationCanvas.width = cropped.width;
    committedAnnotationCanvas.height = cropped.height;
    rotationQuarters = 0;
    annotationCanvas.style.transform = "";
    annotationFrameStyle = "none";
    updateAnnotationFramePreview();
    annotations = [];
    annotationRedoStack = [];
    cropStart = null;
    cropDraft = null;
    setAnnotationTool("arrow");
    clearOcrResultsAfterCrop();
    updateAnnotationEditorLayout(cropped.width, cropped.height);
    setAnnotationZoom(100);
    rebuildCommittedAnnotationCanvas();
    scheduleAnnotationCanvasFit();
  } catch (error) {
    annotationStatus.textContent = `裁剪失败：${String(error)}`;
    console.error("failed to crop selected capture", error);
  } finally {
    cropInProgress = false;
    annotationCropCancel.disabled = false;
    annotationCropApply.disabled = cropDraft === null;
    annotationOcr.disabled = false;
    annotationTranslate.disabled = false;
    annotationPin.disabled = false;
    annotationScrollCapture.disabled = false;
  }
}

async function captureLongScreenshot(): Promise<void> {
  commitTextEditor();
  if (
    !selectedCapture
    || scrollCaptureInProgress
    || cropInProgress
    || frameInProgress
    || ocrInProgress
    || translationInProgress
    || copyInProgress
    || pinInProgress
  ) return;

  if (
    (annotations.length > 0 || rotationQuarters !== 0 || annotationFrameStyle !== "none")
    && !window.confirm("长截图会清除当前标注、旋转和图片边框，是否继续？")
  ) return;

  scrollCaptureInProgress = true;
  annotationScrollCapture.disabled = true;
  annotationOcr.disabled = true;
  annotationTranslate.disabled = true;
  annotationPin.disabled = true;
  annotationFrameSelect.disabled = true;
  annotationStatus.textContent = "正在隐藏编辑器并自动滚动拼接，请暂时不要操作目标窗口…";
  const version = captureSessionVersion;
  let overlayHidden = false;

  try {
    overlayHidden = true;
    const result = await captureScrollingSelection();
    if (version !== captureSessionVersion) return;
    selectedCapture = {
      width: result.width,
      height: result.height,
      cropMs: 0,
    };
    const editorPerformance = await openAnnotationEditor(selectedCapture);
    if (!editorPerformance || version !== captureSessionVersion) return;
    clearOcrResultsAfterCrop();
    await revealCaptureOverlay();
    overlayHidden = false;
    annotationStatus.textContent = `长截图完成：${result.segments} 段 · ${result.width}×${result.height} · ${formatMilliseconds(result.durationMs)}`;
    reportPerformance("scroll-capture", {
      captureAndStitch: result.durationMs,
      selectedPngIpc: editorPerformance.pngIpcMs,
      browserDecode: editorPerformance.decodeMs,
    });
  } catch (error) {
    annotationStatus.textContent = `长截图失败：${String(error)}`;
    console.error("failed to capture scrolling selection", error);
  } finally {
    if (overlayHidden && version === captureSessionVersion) {
      try {
        await revealCaptureOverlay();
      } catch (error) {
        console.error("failed to restore capture overlay after scrolling capture", error);
      }
    }
    if (version === captureSessionVersion) {
      scrollCaptureInProgress = false;
      annotationScrollCapture.disabled = false;
      annotationOcr.disabled = false;
      annotationTranslate.disabled = false;
      annotationPin.disabled = false;
      annotationFrameSelect.disabled = false;
    }
  }
}

async function loadAvailableOcrLanguages(): Promise<void> {
  if (ocrLanguagesLoaded || ocrLanguagesLoading) return;

  ocrLanguagesLoading = true;
  try {
    const languages = await listOcrLanguages();
    const previous = ocrLanguage.value;
    ocrLanguage.replaceChildren();
    const automatic = document.createElement("option");
    automatic.value = "";
    automatic.textContent = "自动（系统）";
    ocrLanguage.append(automatic);
    for (const language of languages) {
      const option = document.createElement("option");
      option.value = language.tag;
      option.textContent = `${language.nativeName} · ${language.tag}`;
      option.title = language.displayName;
      ocrLanguage.append(option);
    }
    if ([...ocrLanguage.options].some((option) => option.value === previous)) {
      ocrLanguage.value = previous;
    }
    ocrLanguagesLoaded = true;
    ocrLanguage.disabled = ocrInProgress || languages.length === 0;
  } catch (error) {
    ocrLanguage.disabled = true;
    console.error("failed to list Windows OCR languages", error);
  } finally {
    ocrLanguagesLoading = false;
  }
}

function selectedTranslationProvider(): TranslationProviderPayload | undefined {
  return translationProviders.find((provider) => provider.provider === settingsProvider.value);
}

function updateTranslationProviderFields(): void {
  const provider = selectedTranslationProvider();
  if (!provider) return;
  settingsApiKeyLabel.textContent = provider.requiresApiKey
    ? `${provider.displayName} API Key`
    : `${provider.displayName} API Key（可选）`;
  settingsApiKey.placeholder = provider.requiresApiKey
    ? `请输入 ${provider.displayName} API Key`
    : "本地服务通常无需 API Key";
}

async function loadAvailableTranslationProviders(): Promise<void> {
  if (translationProviders.length > 0 || translationProvidersLoading) return;

  translationProvidersLoading = true;
  try {
    const providers = await listTranslationProviders();
    translationProviders = providers;
    const previous = settingsProvider.value;
    settingsProvider.replaceChildren();
    for (const provider of providers) {
      const option = document.createElement("option");
      option.value = provider.provider;
      option.textContent = provider.displayName;
      option.title = provider.description;
      settingsProvider.append(option);
    }
    if ([...settingsProvider.options].some((option) => option.value === previous)) {
      settingsProvider.value = previous;
    }
    updateTranslationProviderFields();
  } catch (error) {
    console.error("failed to list translation providers", error);
  } finally {
    translationProvidersLoading = false;
  }
}

async function loadAvailableTranslationModels(): Promise<void> {
  if (translationModelsLoaded || translationModelsLoading) return;

  translationModelsLoading = true;
  try {
    const models = await listTranslationModels();
    const previous = translationModel.value;
    translationModel.replaceChildren();
    for (const model of models) {
      const option = document.createElement("option");
      option.value = model.model;
      option.textContent = model.displayName;
      option.title = `${model.provider} · ${model.model}`;
      translationModel.append(option);
    }
    if ([...translationModel.options].some((option) => option.value === previous)) {
      translationModel.value = previous;
    }
    translationModelsLoaded = true;
  } catch (error) {
    console.error("failed to list translation models", error);
  } finally {
    translationModelsLoading = false;
  }
}

async function openTranslationSettings(): Promise<void> {
  settingsPanel.hidden = false;
  settingsStatus.textContent = "正在读取配置…";
  try {
    await loadAvailableTranslationProviders();
    const config = await getTranslationConfig();
    settingsProvider.value = config.provider;
    updateTranslationProviderFields();
    settingsModel.value = config.model;
    settingsEndpoint.value = config.endpoint;
    settingsApiKey.value = "";
    settingsApiKeyHint.textContent = config.apiKeyConfigured
      ? `已配置（${config.apiKeyHint ?? "已隐藏"}），留空保持不变`
      : "尚未配置";
    settingsClearKey.checked = false;
    const provider = selectedTranslationProvider();
    settingsStatus.textContent = `${provider?.description ?? "翻译服务"}。配置保存在 SnapRust 应用数据目录中。`;
  } catch (error) {
    settingsStatus.textContent = `读取配置失败：${String(error)}`;
  }
}

function closeTranslationSettings(): void {
  settingsRequestVersion += 1;
  cancelActiveTranslationRequest();
  settingsInProgress = false;
  settingsSave.disabled = false;
  settingsTest.disabled = false;
  if (overlay.dataset.state === "settings") {
    void cancel();
  } else {
    settingsPanel.hidden = true;
  }
}

async function saveTranslationSettings(testConnection: boolean): Promise<void> {
  if (settingsInProgress) return;
  const requestVersion = ++settingsRequestVersion;
  settingsInProgress = true;
  settingsSave.disabled = true;
  settingsTest.disabled = true;
  settingsStatus.textContent = "正在保存配置…";
  try {
    const config = await saveTranslationConfig({
      provider: settingsProvider.value,
      apiKey: settingsApiKey.value || undefined,
      clearApiKey: settingsClearKey.checked,
      endpoint: settingsEndpoint.value,
      model: settingsModel.value,
    });
    if (requestVersion !== settingsRequestVersion || settingsPanel.hidden) return;
    settingsApiKey.value = "";
    settingsClearKey.checked = false;
    settingsApiKeyHint.textContent = config.apiKeyConfigured
      ? `已配置（${config.apiKeyHint ?? "已隐藏"}），留空保持不变`
      : "尚未配置";
    translationModelsLoaded = false;
    if (testConnection) {
      settingsStatus.textContent = "配置已保存，正在测试 DeepSeek…";
      const requestId = ++nextTranslationRequestId;
      activeTranslationRequestId = requestId;
      const result = await translateText(
        "Hello, SnapRust",
        "zh-Hans",
        undefined,
        config.model,
        requestId,
      );
      if (
        requestVersion !== settingsRequestVersion
        || activeTranslationRequestId !== requestId
        || settingsPanel.hidden
      ) return;
      activeTranslationRequestId = null;
      settingsStatus.textContent = `测试成功：${result.text}`;
    } else {
      settingsStatus.textContent = "配置已保存，下次翻译时生效。";
    }
  } catch (error) {
    if (requestVersion === settingsRequestVersion && !settingsPanel.hidden) {
      settingsStatus.textContent = `保存或测试失败：${String(error)}`;
    }
  } finally {
    if (requestVersion === settingsRequestVersion) {
      settingsInProgress = false;
      settingsSave.disabled = false;
      settingsTest.disabled = false;
    }
  }
}

function hideImageContextMenu(): void {
  imageContextMenu.hidden = true;
}

function showImageContextMenu(clientX: number, clientY: number): void {
  imageContextMenu.hidden = false;
  const margin = 8;
  const left = Math.min(clientX, window.innerWidth - imageContextMenu.offsetWidth - margin);
  const top = Math.min(clientY, window.innerHeight - imageContextMenu.offsetHeight - margin);
  imageContextMenu.style.left = `${Math.max(margin, left)}px`;
  imageContextMenu.style.top = `${Math.max(margin, top)}px`;
}

async function rotateSelectedImage(deltaQuarters: number): Promise<void> {
  if (
    !selectedCapture
    || scrollCaptureInProgress
    || cropInProgress
    || frameInProgress
    || ocrInProgress
    || translationInProgress
    || copyInProgress
    || pinInProgress
  ) return;
  try {
    const rotation = await rotateSelectedCapture(deltaQuarters);
    rotationQuarters = rotation.quarters;
    annotationCanvas.style.transform = rotationQuarters === 0
      ? ""
      : `rotate(${rotationQuarters * 90}deg)`;
    annotationStatus.textContent = `已旋转 ${rotationQuarters * 90}°；复制或钉图时会输出旋转后的图片`;
  } catch (error) {
    annotationStatus.textContent = `旋转失败：${String(error)}`;
  }
}

async function handleImageContextAction(action: string): Promise<void> {
  hideImageContextMenu();
  switch (action) {
    case "copy":
      await copySelection();
      break;
    case "pin":
      commitTextEditor();
      await pinSelection();
      break;
    case "ocr":
      commitTextEditor();
      await recognizeSelection();
      break;
    case "translate":
      await translateSelection();
      break;
    case "crop":
      if (!scrollCaptureInProgress && !cropInProgress && !ocrInProgress && !translationInProgress && !copyInProgress && !pinInProgress) {
        setAnnotationTool("crop");
      }
      break;
    case "rotate-left":
      await rotateSelectedImage(-1);
      break;
    case "rotate-right":
      await rotateSelectedImage(1);
      break;
    case "reset-rotation":
      await rotateSelectedImage(-rotationQuarters);
      break;
    case "destroy":
      await cancel();
      break;
    default:
      break;
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
    ? `${physical.width} × ${physical.height}`
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
    const editorPerformance = await openAnnotationEditor(selected);
    if (!editorPerformance) return;
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
    console.error("failed to crop capture selection", error);
  }
}

async function copySelection(): Promise<void> {
  commitTextEditor();
  if (!selectedCapture || copyInProgress || pinInProgress || ocrInProgress || frameInProgress || scrollCaptureInProgress) {
    return;
  }

  copyInProgress = true;
  annotationPin.disabled = true;
  annotationOcr.disabled = true;
  annotationTranslate.disabled = true;
  annotationScrollCapture.disabled = true;
  const totalStarted = performance.now();
  let annotationSyncMs = 0;
  if (!annotationEditor.hidden) annotationStatus.textContent = "正在由 Rust 合成并复制…";

  try {
    if (!annotationEditor.hidden && annotations.length > 0) {
      const annotationStarted = performance.now();
      await setCaptureAnnotations(annotations);
      annotationSyncMs = performance.now() - annotationStarted;
    }
    const ocrText = ocrPanel.hidden ? undefined : ocrResult.value.trim() || undefined;
    const copied = await copySelectedCapture(ocrText);
    reportPerformance("copy", {
      annotationSync: annotationSyncMs,
      rustRender: copied.renderMs,
      clipboard: copied.clipboardMs,
      rustTotal: copied.totalMs,
      endToEnd: performance.now() - totalStarted,
    });
    resetSelectionUi();
  } catch (error) {
    if (!annotationEditor.hidden) annotationStatus.textContent = `复制失败：${String(error)}`;
    console.error("failed to copy selected capture", error);
  } finally {
    copyInProgress = false;
    annotationPin.disabled = false;
    annotationOcr.disabled = false;
    annotationTranslate.disabled = false;
    annotationScrollCapture.disabled = false;
  }
}

async function pinSelection(): Promise<void> {
  if (!selectedCapture || pinInProgress || copyInProgress || ocrInProgress || frameInProgress || scrollCaptureInProgress) {
    return;
  }

  pinInProgress = true;
  annotationPin.disabled = true;
  annotationOcr.disabled = true;
  annotationTranslate.disabled = true;
  annotationScrollCapture.disabled = true;
  const totalStarted = performance.now();
  let annotationSyncMs = 0;
  annotationStatus.textContent = "正在由 Rust 合成并创建置顶窗口…";

  try {
    if (annotations.length > 0) {
      const annotationStarted = performance.now();
      await setCaptureAnnotations(annotations);
      annotationSyncMs = performance.now() - annotationStarted;
    }
    const ocrText = ocrPanel.hidden ? undefined : ocrResult.value.trim() || undefined;
    const pinned = await pinSelectedCapture(ocrText);
    reportPerformance("pin", {
      annotationSync: annotationSyncMs,
      rustRender: pinned.renderMs,
      pngEncode: pinned.pngEncodeMs,
      windowCreate: pinned.windowCreateMs,
      rustTotal: pinned.totalMs,
      endToEnd: performance.now() - totalStarted,
    });
    resetSelectionUi();
  } catch (error) {
    annotationStatus.textContent = `钉图失败：${String(error)}`;
    console.error("failed to pin selected capture", error);
  } finally {
    pinInProgress = false;
    annotationPin.disabled = false;
    annotationOcr.disabled = false;
    annotationTranslate.disabled = false;
    annotationScrollCapture.disabled = false;
  }
}

async function recognizeSelection(): Promise<boolean> {
  if (
    !selectedCapture
    || ocrInProgress
    || copyInProgress
    || pinInProgress
    || translationInProgress
    || frameInProgress
    || scrollCaptureInProgress
  ) return false;

  ocrInProgress = true;
  const version = captureSessionVersion;
  annotationOcr.disabled = true;
  annotationPin.disabled = true;
  annotationTranslate.disabled = true;
  annotationScrollCapture.disabled = true;
  ocrPanel.hidden = false;
  updateAnnotationEditorLayout();
  scheduleAnnotationCanvasFit();
  ocrMeta.textContent = "正在识别…";
  ocrResult.value = "";
  renderOcrLines([]);
  ocrCopy.disabled = true;
  translationResult.value = "";
  translationMeta.textContent = "等待 OCR 结果";
  translationCopy.disabled = true;
  annotationStatus.textContent = "正在使用 Windows 本地 OCR 识别原始选区…";

  try {
    const result = await recognizeSelectedCapture(ocrLanguage.value || undefined);
    if (version !== captureSessionVersion) return false;
    ocrResult.value = result.text;
    renderOcrLines(result.lines);
    ocrCopy.disabled = result.text.trim().length === 0;
    const resized = result.sourceWidth !== result.recognitionWidth || result.sourceHeight !== result.recognitionHeight;
    const sizeText = resized
      ? `${result.sourceWidth}×${result.sourceHeight} → ${result.recognitionWidth}×${result.recognitionHeight}`
      : `${result.sourceWidth}×${result.sourceHeight}`;
    ocrMeta.textContent = `${result.language} · ${result.lineCount} 行 · ${sizeText} · ${formatMilliseconds(result.durationMs)}`;
    annotationStatus.textContent = result.text.trim()
      ? `OCR 完成：识别到 ${result.lineCount} 行文字`
      : "OCR 完成：选区中没有识别到文字";
    reportPerformance("ocr", { windowsOcr: result.durationMs });
    return result.text.trim().length > 0;
  } catch (error) {
    if (version !== captureSessionVersion) return false;
    ocrMeta.textContent = "识别失败";
    ocrResult.value = `OCR 失败：${String(error)}`;
    renderOcrLines([]);
    annotationStatus.textContent = "OCR 失败；请确认 Windows 已安装当前语言的 OCR 组件";
    console.error("failed to recognize selected capture", error);
    return false;
  } finally {
    if (version === captureSessionVersion) {
      ocrInProgress = false;
      annotationOcr.disabled = false;
      annotationPin.disabled = false;
      annotationTranslate.disabled = false;
      annotationScrollCapture.disabled = false;
      ocrLanguage.disabled = !ocrLanguagesLoaded || ocrLanguage.options.length <= 1;
    }
  }
}

async function copyOcrResult(): Promise<void> {
  const text = ocrResult.value;
  if (!text.trim() || ocrCopy.disabled) return;

  ocrCopy.disabled = true;
  try {
    await copyText(text);
    ocrCopy.textContent = "已复制";
    annotationStatus.textContent = "OCR 文字已复制到剪贴板";
  } catch (error) {
    ocrCopy.textContent = "复制失败";
    annotationStatus.textContent = `OCR 文字复制失败：${String(error)}`;
  } finally {
    window.setTimeout(() => {
      ocrCopy.textContent = "复制文字";
      ocrCopy.disabled = !ocrResult.value.trim();
    }, 1_200);
  }
}

async function translateSelection(): Promise<void> {
  if (!selectedCapture || translationInProgress || copyInProgress || pinInProgress || scrollCaptureInProgress) return;

  commitTextEditor();
  annotationTranslate.disabled = true;
  const hasOcrText = ocrResult.value.trim().length > 0;
  if (!hasOcrText) {
    const recognized = await recognizeSelection();
    if (!recognized || !selectedCapture) {
      annotationTranslate.disabled = false;
      return;
    }
  }

  translationInProgress = true;
  ocrPanel.hidden = false;
  updateAnnotationEditorLayout();
  scheduleAnnotationCanvasFit();
  translationRun.disabled = true;
  translationCopy.disabled = true;
  translationResult.value = "";
  translationMeta.textContent = "正在翻译…";
  annotationOcr.disabled = true;
  annotationPin.disabled = true;
  annotationStatus.textContent = "正在翻译 OCR 文字…";
  annotationScrollCapture.disabled = true;
  const version = captureSessionVersion;
  const requestId = ++nextTranslationRequestId;
  activeTranslationRequestId = requestId;

  try {
    const translated = await translateText(
      ocrResult.value,
      translationTarget.value,
      undefined,
      translationModel.value || undefined,
      requestId,
    );
    if (version !== captureSessionVersion || activeTranslationRequestId !== requestId) return;
    activeTranslationRequestId = null;
    translationResult.value = translated.text;
    translationCopy.disabled = translated.text.trim().length === 0;
    translationMeta.textContent = `${translated.provider}/${translated.model} · ${translated.sourceLanguage ?? "自动"} → ${translated.targetLanguage} · ${formatMilliseconds(translated.durationMs)}`;
    annotationStatus.textContent = "翻译完成，可复制译文";
    reportPerformance("translation", { translator: translated.durationMs });
  } catch (error) {
    if (version !== captureSessionVersion || activeTranslationRequestId !== requestId) return;
    translationMeta.textContent = "翻译失败";
    translationResult.value = `翻译失败：${String(error)}`;
    annotationStatus.textContent = "翻译失败；请检查翻译服务配置和网络连接";
    console.error("failed to translate OCR result", error);
  } finally {
    if (activeTranslationRequestId === requestId) activeTranslationRequestId = null;
    if (version === captureSessionVersion) {
      translationInProgress = false;
      translationRun.disabled = false;
      annotationTranslate.disabled = false;
      annotationOcr.disabled = false;
      annotationPin.disabled = false;
      annotationScrollCapture.disabled = false;
    }
  }
}

async function copyTranslationResult(): Promise<void> {
  const text = translationResult.value;
  if (!text.trim() || translationCopy.disabled) return;

  translationCopy.disabled = true;
  try {
    await copyText(text);
    translationCopy.textContent = "已复制";
    annotationStatus.textContent = "译文已复制到剪贴板";
  } catch (error) {
    translationCopy.textContent = "复制失败";
    annotationStatus.textContent = `译文复制失败：${String(error)}`;
  } finally {
    window.setTimeout(() => {
      translationCopy.textContent = "复制译文";
      translationCopy.disabled = !translationResult.value.trim();
    }, 1_200);
  }
}

window.addEventListener("keydown", (event) => {
  if (!settingsPanel.hidden && event.key === "Escape") {
    event.preventDefault();
    closeTranslationSettings();
    return;
  }
  if (!imageContextMenu.hidden && event.key === "Escape") {
    event.preventDefault();
    hideImageContextMenu();
    return;
  }
  if (!historyPanel.hidden && event.key === "Escape") {
    event.preventDefault();
    void closeHistory();
    return;
  }
  if (annotationTextEditor.contains(event.target as Node)) return;
  if (ocrPanel.contains(event.target as Node) && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "c") return;

  if (!annotationEditor.hidden && annotationTool === "crop") {
    if (event.key === "Escape") {
      event.preventDefault();
      setAnnotationTool("arrow");
      return;
    }
    if (event.key === "Enter" && cropDraft) {
      event.preventDefault();
      void applyCropSelection();
      return;
    }
  }

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

  if (!annotationEditor.hidden && (event.ctrlKey || event.metaKey)) {
    if (event.key === "+" || event.key === "=") {
      event.preventDefault();
      setAnnotationZoom(Number(annotationZoom.value) + 10);
      return;
    }
    if (event.key === "-") {
      event.preventDefault();
      setAnnotationZoom(Number(annotationZoom.value) - 10);
      return;
    }
    if (event.key === "0") {
      event.preventDefault();
      setAnnotationZoom(100);
      scheduleAnnotationCanvasFit();
      return;
    }
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
  if (
    annotationTextEditor.contains(event.target as Node)
    || settingsPanel.contains(event.target as Node)
    || historyPanel.contains(event.target as Node)
  ) return;
  if (selectedCapture && annotationCanvasWrap.contains(event.target as Node)) {
    event.preventDefault();
    showImageContextMenu(event.clientX, event.clientY);
    return;
  }
  event.preventDefault();
  void cancel();
});

window.addEventListener("pointerdown", (event) => {
  if (!imageContextMenu.contains(event.target as Node)) hideImageContextMenu();
});

overlay.addEventListener("pointerdown", (event) => {
  if (!captureReady || !annotationEditor.hidden || event.button !== 0 || copyInProgress) {
    return;
  }

  dragStart = pointFromEvent(event);
  activePointerId = event.pointerId;
  selectedCapture = null;
  currentSelection = createSelection(dragStart, dragStart);
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
  dragStart = null;
  activePointerId = null;
  if (overlay.hasPointerCapture(event.pointerId)) {
    overlay.releasePointerCapture(event.pointerId);
  }

  currentSelection = selection;
  renderSelection(selection);
  if (!selectionHasArea(selection)) {
    selectedCapture = null;
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
});

document.querySelectorAll<HTMLButtonElement>("[data-tool]").forEach((button) => {
  button.addEventListener("click", () => {
    if (
      button.dataset.tool === "crop"
      && (cropInProgress || ocrInProgress || translationInProgress || copyInProgress || pinInProgress)
    ) return;
    setAnnotationTool(button.dataset.tool as AnnotationTool);
  });
});

settingsProvider.addEventListener("change", () => {
  const provider = selectedTranslationProvider();
  if (!provider) return;
  settingsModel.value = provider.defaultModel;
  settingsEndpoint.value = provider.defaultEndpoint;
  settingsApiKey.value = "";
  settingsClearKey.checked = false;
  settingsApiKeyHint.textContent = provider.requiresApiKey
    ? "保存后将使用该提供商的密钥"
    : "该提供商通常不需要 API Key";
  updateTranslationProviderFields();
  settingsStatus.textContent = `${provider.description}。请检查模型和端点后保存。`;
  translationModelsLoaded = false;
});
settingsClose.addEventListener("pointerdown", (event) => {
  event.preventDefault();
  event.stopPropagation();
  closeTranslationSettings();
});
settingsClose.addEventListener("click", (event) => {
  event.stopPropagation();
  closeTranslationSettings();
});
settingsPanel.addEventListener("pointerdown", (event) => event.stopPropagation());
settingsPanel.addEventListener("click", (event) => event.stopPropagation());
translationSettingsForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void saveTranslationSettings(false);
});
settingsTest.addEventListener("click", () => void saveTranslationSettings(true));
settingsClearKey.addEventListener("change", () => {
  if (settingsClearKey.checked) settingsApiKey.value = "";
});
imageContextMenu.querySelectorAll<HTMLButtonElement>("[data-image-action]").forEach((button) => {
  button.addEventListener("click", () => void handleImageContextAction(button.dataset.imageAction ?? ""));
});

annotationWidth.addEventListener("input", () => {
  annotationWidthValue.textContent = `${annotationWidth.value} px`;
});

annotationFrameSelect.addEventListener("change", () => {
  void setAnnotationFrameStyle(annotationFrameSelect.value as FrameStyle);
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

annotationCropCancel.addEventListener("click", () => setAnnotationTool("arrow"));
annotationCropApply.addEventListener("click", () => void applyCropSelection());

annotationScrollCapture.addEventListener("click", () => void captureLongScreenshot());

annotationPin.addEventListener("click", () => {
  commitTextEditor();
  void pinSelection();
});

annotationOcr.addEventListener("click", () => {
  commitTextEditor();
  void recognizeSelection();
});

annotationTranslate.addEventListener("click", () => void translateSelection());

ocrCopy.addEventListener("click", () => void copyOcrResult());
translationRun.addEventListener("click", () => void translateSelection());
translationCopy.addEventListener("click", () => void copyTranslationResult());

function markTranslationSettingsChanged(): void {
  if (!translationResult.value.trim()) return;
  translationMeta.textContent = "目标语言或模型已修改，请重新翻译";
  translationCopy.disabled = true;
}

translationTarget.addEventListener("change", markTranslationSettingsChanged);
translationModel.addEventListener("change", markTranslationSettingsChanged);

ocrResult.addEventListener("input", () => {
  ocrCopy.disabled = !ocrResult.value.trim();
  ocrCopy.textContent = "复制文字";
  translationResult.value = "";
  translationMeta.textContent = "文字已修改，请重新翻译";
  translationCopy.disabled = true;
  translationCopy.textContent = "复制译文";
});

ocrLanguage.addEventListener("change", () => {
  if (!ocrPanel.hidden) void recognizeSelection();
});

ocrClose.addEventListener("click", () => {
  ocrPanel.hidden = true;
  updateAnnotationEditorLayout();
  scheduleAnnotationCanvasFit();
  renderOcrLines([]);
});

annotationZoom.addEventListener("input", () => setAnnotationZoom(Number(annotationZoom.value)));
annotationZoomOut.addEventListener("click", () => setAnnotationZoom(Number(annotationZoom.value) - 10));
annotationZoomIn.addEventListener("click", () => setAnnotationZoom(Number(annotationZoom.value) + 10));
annotationZoomFit.addEventListener("click", () => {
  setAnnotationZoom(100);
  scheduleAnnotationCanvasFit();
});

annotationCanvasWrap.addEventListener("wheel", (event) => {
  if (annotationEditor.hidden || (!event.ctrlKey && !event.metaKey)) return;
  event.preventDefault();
  setAnnotationZoom(Number(annotationZoom.value) + (event.deltaY < 0 ? 10 : -10));
}, { passive: false });

window.addEventListener("resize", () => {
  if (!annotationEditor.hidden) updateAnnotationEditorLayout();
  if (!annotationEditor.hidden) scheduleAnnotationCanvasFit();
});

historyClose.addEventListener("pointerdown", (event) => {
  event.preventDefault();
  event.stopPropagation();
  void closeHistory();
});
historyClose.addEventListener("click", (event) => {
  event.stopPropagation();
  void closeHistory();
});
historyPanel.addEventListener("pointerdown", (event) => event.stopPropagation());
historyPanel.addEventListener("click", (event) => event.stopPropagation());
historyPanel.addEventListener("contextmenu", (event) => {
  event.preventDefault();
  event.stopPropagation();
});

historySearch.addEventListener("input", () => {
  if (historySearchTimer !== null) window.clearTimeout(historySearchTimer);
  historySearchTimer = window.setTimeout(() => {
    historySearchTimer = null;
    void loadHistory();
  }, 180);
});

historyFavorites.addEventListener("change", () => void loadHistory());

historySelectAll.addEventListener("change", () => {
  selectedHistoryIds.clear();
  if (historySelectAll.checked) {
    historyVisibleIds.forEach((id) => selectedHistoryIds.add(id));
  }
  document.querySelectorAll<HTMLInputElement>(".history-card__select input").forEach((input) => {
    input.checked = selectedHistoryIds.has(Number(input.dataset.historyId));
  });
  updateHistoryBatchControls();
});

historyBatchFavorite.addEventListener("click", () => void setHistoryFavoritesInBatch(true));
historyBatchUnfavorite.addEventListener("click", () => void setHistoryFavoritesInBatch(false));
historyBatchExport.addEventListener("click", () => void exportHistoryInBatch());
historyBatchDelete.addEventListener("click", () => void removeHistoryInBatch());

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
  if (annotationTool === "crop") {
    if (cropInProgress) return;
    annotationStart = null;
    annotationDraft = null;
    cropStart = point;
    cropDraft = annotationRect(point, point);
    annotationPointerId = event.pointerId;
    annotationCanvas.setPointerCapture(event.pointerId);
    annotationCropApply.disabled = true;
    scheduleAnnotationRender();
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
  if (annotationTool === "crop" && cropStart && event.pointerId === annotationPointerId) {
    event.stopPropagation();
    cropDraft = annotationRect(cropStart, annotationPointFromEvent(event));
    annotationCropApply.disabled = cropDraft.width < 1 || cropDraft.height < 1 || cropInProgress;
    scheduleAnnotationRender();
    return;
  }
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
  if (annotationTool === "crop" && cropStart && event.pointerId === annotationPointerId) {
    event.stopPropagation();
    cropDraft = annotationRect(cropStart, annotationPointFromEvent(event));
    cropStart = null;
    annotationPointerId = null;
    if (annotationCanvas.hasPointerCapture(event.pointerId)) annotationCanvas.releasePointerCapture(event.pointerId);
    annotationCropApply.disabled = cropDraft.width < 1 || cropDraft.height < 1 || cropInProgress;
    scheduleAnnotationRender();
    return;
  }
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
  if (annotationTool === "crop") {
    cropStart = null;
    cropDraft = null;
    annotationPointerId = null;
    annotationCropApply.disabled = true;
    scheduleAnnotationRender();
    return;
  }
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
  captureReady = false;
  currentCapture = null;

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
    const decodeMs = performance.now() - decodeStarted;

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
    console.error("failed to load current screen capture", error);
  }
});

await listen("history://show", () => {
  resetSelectionUi();
  overlay.dataset.state = "history";
  historyPanel.hidden = false;
  void loadHistory();
  window.requestAnimationFrame(() => historySearch.focus());
});

await listen("settings://show", () => {
  resetSelectionUi();
  overlay.dataset.state = "settings";
  void openTranslationSettings();
  window.requestAnimationFrame(() => settingsApiKey.focus());
});

window.addEventListener("beforeunload", () => {
  releaseImageResources();
  releaseHistoryThumbnails();
});
