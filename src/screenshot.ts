import { invoke } from "@tauri-apps/api/core";

import type { PhysicalSelectionRect } from "./selection";

export type CapturePayload = Readonly<{
  width: number;
  height: number;
  captureMs: number;
  desktop: Readonly<{
    x: number;
    y: number;
    width: number;
    height: number;
    monitors: ReadonlyArray<Readonly<{
      index: number;
      x: number;
      y: number;
      width: number;
      height: number;
      dpiX: number;
      dpiY: number;
      scaleFactor: number;
      isPrimary: boolean;
    }>>;
  }>;
}>;

export type SelectionPayload = Readonly<{
  width: number;
  height: number;
  cropMs: number;
}>;

export type SelectionCropRect = Readonly<{
  x: number;
  y: number;
  width: number;
  height: number;
}>;

export type RotationPayload = Readonly<{
  quarters: number;
  width: number;
  height: number;
}>;

export type FrameStyle = "none" | "macos";

export type CopyPayload = Readonly<{
  width: number;
  height: number;
  renderMs: number;
  clipboardMs: number;
  totalMs: number;
}>;

export type PinCreatedPayload = Readonly<{
  label: string;
  width: number;
  height: number;
  renderMs: number;
  pngEncodeMs: number;
  windowCreateMs: number;
  totalMs: number;
}>;

export type PinMetadata = Readonly<{
  width: number;
  height: number;
}>;

export type OcrPayload = Readonly<{
  text: string;
  language: string;
  sourceWidth: number;
  sourceHeight: number;
  recognitionWidth: number;
  recognitionHeight: number;
  lineCount: number;
  lines: ReadonlyArray<OcrLinePayload>;
  durationMs: number;
}>;

export type OcrRectPayload = Readonly<{
  x: number;
  y: number;
  width: number;
  height: number;
}>;

export type OcrLinePayload = Readonly<{
  text: string;
  rect: OcrRectPayload;
}>;

export type OcrLanguagePayload = Readonly<{
  tag: string;
  displayName: string;
  nativeName: string;
}>;

export type TranslationPayload = Readonly<{
  text: string;
  sourceLanguage: string | null;
  targetLanguage: string;
  provider: string;
  model: string;
  durationMs: number;
}>;

export type TranslationModelPayload = Readonly<{
  provider: string;
  model: string;
  displayName: string;
}>;

export type TranslationProviderPayload = Readonly<{
  provider: string;
  displayName: string;
  defaultEndpoint: string;
  defaultModel: string;
  requiresApiKey: boolean;
  description: string;
}>;

export type TranslationConfigPayload = Readonly<{
  provider: string;
  apiKeyConfigured: boolean;
  apiKeyHint: string | null;
  endpoint: string;
  model: string;
}>;

export type HistoryItemPayload = Readonly<{
  id: number;
  width: number;
  height: number;
  createdAtMs: number;
  favorite: boolean;
  ocrText: string | null;
  tags: ReadonlyArray<string>;
}>;

export type HistoryUsagePayload = Readonly<{
  itemCount: number;
  imageBytes: number;
  maxItems: number;
  maxImageBytes: number;
}>;

export type HistoryExportPayload = Readonly<{
  directory: string;
  exportedCount: number;
}>;

export type AnnotationPoint = Readonly<{ x: number; y: number }>;
export type AnnotationRect = Readonly<{
  x: number;
  y: number;
  width: number;
  height: number;
}>;
export type Annotation =
  | Readonly<{ kind: "arrow"; start: AnnotationPoint; end: AnnotationPoint; color: string; width: number }>
  | Readonly<{ kind: "rectangle"; rect: AnnotationRect; color: string; width: number }>
  | Readonly<{ kind: "ellipse"; rect: AnnotationRect; color: string; width: number }>
  | Readonly<{ kind: "brush"; points: ReadonlyArray<AnnotationPoint>; color: string; width: number }>
  | Readonly<{ kind: "mosaic"; rect: AnnotationRect; blockSize: number }>
  | Readonly<{ kind: "text"; position: AnnotationPoint; text: string; color: string; fontSize: number }>;

export async function showCaptureOverlay(): Promise<void> {
  await invoke("show_capture_overlay");
}

export async function revealCaptureOverlay(): Promise<void> {
  await invoke("reveal_capture_overlay");
}

export async function cancelCapture(): Promise<void> {
  await invoke("cancel_capture");
}

export async function getCurrentCapture(): Promise<CapturePayload> {
  return invoke<CapturePayload>("get_current_capture");
}

export async function getCurrentCaptureImage(): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("get_current_capture_image");
}

export async function selectCaptureRegion(
  selection: PhysicalSelectionRect,
): Promise<SelectionPayload> {
  return invoke<SelectionPayload>("select_capture_region", { selection });
}

export async function cropSelectedCapture(crop: SelectionCropRect): Promise<SelectionPayload> {
  return invoke<SelectionPayload>("crop_selected_capture", { crop });
}

export async function getSelectedCaptureImage(): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("get_selected_capture_image");
}

export async function setCaptureAnnotations(annotations: ReadonlyArray<Annotation>): Promise<void> {
  await invoke("set_capture_annotations", { annotations });
}

export async function setCaptureFrame(style: FrameStyle): Promise<void> {
  await invoke("set_capture_frame", { style });
}

export async function rotateSelectedCapture(deltaQuarters: number): Promise<RotationPayload> {
  return invoke<RotationPayload>("rotate_selected_capture", { deltaQuarters });
}

export async function recognizeSelectedCapture(language?: string): Promise<OcrPayload> {
  return invoke<OcrPayload>("recognize_selected_capture", { language });
}

export async function listOcrLanguages(): Promise<ReadonlyArray<OcrLanguagePayload>> {
  return invoke<ReadonlyArray<OcrLanguagePayload>>("list_ocr_languages");
}

export async function listTranslationProviders(): Promise<ReadonlyArray<TranslationProviderPayload>> {
  return invoke<ReadonlyArray<TranslationProviderPayload>>("list_translation_providers");
}

export async function listTranslationModels(provider?: string): Promise<ReadonlyArray<TranslationModelPayload>> {
  return invoke<ReadonlyArray<TranslationModelPayload>>("list_translation_models", { provider });
}

export async function getTranslationConfig(): Promise<TranslationConfigPayload> {
  return invoke<TranslationConfigPayload>("get_translation_config");
}

export async function saveTranslationConfig(config: {
  provider: string;
  apiKey?: string;
  clearApiKey: boolean;
  endpoint: string;
  model: string;
}): Promise<TranslationConfigPayload> {
  return invoke<TranslationConfigPayload>("save_translation_config", { config });
}

export async function translateText(
  text: string,
  targetLanguage: string,
  sourceLanguage?: string,
  model?: string,
  requestId?: number,
): Promise<TranslationPayload> {
  return invoke<TranslationPayload>("translate_text", {
    text,
    targetLanguage,
    sourceLanguage,
    model,
    requestId,
  });
}

export async function cancelTranslation(requestId: number): Promise<void> {
  await invoke("cancel_translation", { requestId });
}

export async function copyText(text: string): Promise<void> {
  await invoke("copy_text", { text });
}

export async function copySelectedCapture(ocrText?: string): Promise<CopyPayload> {
  return invoke<CopyPayload>("copy_selected_capture", { ocrText });
}

export async function pinSelectedCapture(ocrText?: string): Promise<PinCreatedPayload> {
  return invoke<PinCreatedPayload>("pin_selected_capture", { ocrText });
}

export async function listHistory(
  query: string | undefined,
  favoritesOnly: boolean,
): Promise<ReadonlyArray<HistoryItemPayload>> {
  return invoke<ReadonlyArray<HistoryItemPayload>>("list_history", { query, favoritesOnly });
}

export async function getHistoryUsage(): Promise<HistoryUsagePayload> {
  return invoke<HistoryUsagePayload>("get_history_usage");
}

export async function getHistoryThumbnail(id: number): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("get_history_thumbnail", { id });
}

export async function copyHistoryCapture(id: number): Promise<void> {
  await invoke("copy_history_capture", { id });
}

export async function pinHistoryCapture(id: number): Promise<PinCreatedPayload> {
  return invoke<PinCreatedPayload>("pin_history_capture", { id });
}

export async function setHistoryFavorite(id: number, favorite: boolean): Promise<void> {
  await invoke("set_history_favorite", { id, favorite });
}

export async function setHistoryTags(id: number, tags: ReadonlyArray<string>): Promise<void> {
  await invoke("set_history_tags", { id, tags });
}

export async function setHistoryFavoriteBatch(
  ids: ReadonlyArray<number>,
  favorite: boolean,
): Promise<void> {
  await invoke("set_history_favorite_batch", { ids, favorite });
}

export async function exportHistoryCaptures(
  ids: ReadonlyArray<number>,
): Promise<HistoryExportPayload> {
  return invoke<HistoryExportPayload>("export_history_captures", { ids });
}

export async function deleteHistoryCapture(id: number): Promise<void> {
  await invoke("delete_history_capture", { id });
}

export async function deleteHistoryCaptures(ids: ReadonlyArray<number>): Promise<void> {
  await invoke("delete_history_captures", { ids });
}

export async function hideHistoryWindow(): Promise<void> {
  await invoke("hide_history_window");
}

export async function getPinnedCapture(label: string): Promise<PinMetadata> {
  return invoke<PinMetadata>("get_pinned_capture", { label });
}

export async function getPinnedCaptureImage(label: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("get_pinned_capture_image", { label });
}

export async function warmupPinWindow(label: string): Promise<void> {
  await invoke("warmup_pin_window", { label });
}

export async function revealPinWindow(label: string): Promise<void> {
  await invoke("reveal_pin_window", { label });
}

export async function setPinWindowGeometry(
  label: string,
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<void> {
  await invoke("set_pin_window_geometry", { label, x, y, width, height });
}

export async function setPinOpacity(label: string, opacity: number): Promise<void> {
  await invoke("set_pin_opacity", { label, opacity });
}

export async function closePin(label: string): Promise<void> {
  await invoke("close_pin", { label });
}
