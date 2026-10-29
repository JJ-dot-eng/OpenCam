// 켤 때 GitHub 릴리스에서 새 버전 확인. 새 버전이 있으면 알리고 받는 곳만 열어 준다(받기·설치는 사용자가 직접).

import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { confirmAsk } from "./ask";

const LATEST_API = "https://api.github.com/repos/JJ-dot-eng/OpenCam/releases/latest";

/** "v0.1.10" → [0, 1, 10] */
function parts(v: string): number[] {
  return v.replace(/^v/, "").split(".").map((n) => parseInt(n, 10) || 0);
}

function isNewer(latest: string, current: string): boolean {
  const a = parts(latest), b = parts(current);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0);
  }
  return false;
}

export async function checkForUpdate() {
  let latest: { tag_name: string; html_url: string };
  try {
    const res = await fetch(LATEST_API, { headers: { Accept: "application/vnd.github+json" } });
    if (!res.ok) return;
    latest = await res.json();
  } catch {
    return; // 인터넷이 없거나 GitHub에 닿지 않으면 조용히 넘어간다
  }
  const current = await getVersion();
  if (!latest.tag_name || !isNewer(latest.tag_name, current)) return;
  const version = latest.tag_name.replace(/^v/, "");
  if (await confirmAsk(`새 버전 ${version}이 나왔습니다. 받는 곳을 열까요?\n(지금 버전 ${current})`, "열기")) {
    await openUrl(latest.html_url);
  }
}
