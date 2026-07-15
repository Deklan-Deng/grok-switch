import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateProgress = {
  downloaded: number;
  total: number | null;
};

export type UpdateInfo = {
  version: string;
  currentVersion: string;
  notes: string | null;
  date: string | null;
};

/** Map raw plugin/network errors to short Chinese copy for the status bar. */
export function formatUpdateError(err: unknown): string {
  const raw = String(err ?? "");
  const lower = raw.toLowerCase();
  if (
    lower.includes("could not fetch") ||
    lower.includes("error sending request") ||
    lower.includes("failed to fetch") ||
    lower.includes("404") ||
    lower.includes("not found")
  ) {
    return "暂时无法获取更新信息（网络或尚未发布 latest.json）";
  }
  if (lower.includes("signature") || lower.includes("minisign")) {
    return "更新签名校验失败，已中止安装";
  }
  if (raw.length > 120) {
    return `检查更新失败：${raw.slice(0, 120)}…`;
  }
  return `检查更新失败：${raw}`;
}

export async function checkForUpdate(): Promise<Update | null> {
  return check();
}

export function toUpdateInfo(update: Update): UpdateInfo {
  return {
    version: update.version,
    currentVersion: update.currentVersion,
    notes: update.body ?? null,
    date: update.date ?? null,
  };
}

export async function downloadAndInstallUpdate(
  update: Update,
  onProgress?: (p: UpdateProgress) => void,
): Promise<void> {
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength ?? null;
        downloaded = 0;
        onProgress?.({ downloaded, total });
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress?.({ downloaded, total });
        break;
      case "Finished":
        onProgress?.({ downloaded, total });
        break;
    }
  });
}

export async function relaunchApp(): Promise<void> {
  await relaunch();
}
