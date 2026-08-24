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

export async function getSelectedCaptureImage(): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("get_selected_capture_image");
}

export async function setCaptureAnnotations(annotations: ReadonlyArray<Annotation>): Promise<void> {
  await invoke("set_capture_annotations", { annotations });
}

export async function copySelectedCapture(): Promise<CopyPayload> {
  return invoke<CopyPayload>("copy_selected_capture");
}

export async function pinSelectedCapture(): Promise<PinCreatedPayload> {
  return invoke<PinCreatedPayload>("pin_selected_capture");
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

export async function setPinOpacity(label: string, opacity: number): Promise<void> {
  await invoke("set_pin_opacity", { label, opacity });
}

export async function closePin(label: string): Promise<void> {
  await invoke("close_pin", { label });
}
