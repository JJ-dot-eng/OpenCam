// 켤 때 GitHub 릴리스에서 새 버전 확인.
// 설치 버전은 받아서 설치한 뒤 다시 켜고, 포터블은 스스로 바꿀 수 없으니 받는 곳만 알려 준다.

import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { api } from "./api";
import { alertAsk, confirmAsk } from "./ask";

const RELEASES_URL = "https://github.com/JJ-dot-eng/OpenCam/releases/latest";

export async function checkForUpdate(notify: (message: string) => void) {
  let update;
  try {
    update = await check();
  } catch {
    return; // 인터넷이 없거나 GitHub에 닿지 않으면 조용히 넘어간다
  }
  if (!update) return;

  if (!(await api.isInstalled())) {
    if (await confirmAsk(`새 버전 ${update.version}이 나왔습니다. 받는 곳을 열까요?`, "열기")) await openUrl(RELEASES_URL);
    return;
  }
  if (!(await confirmAsk(`새 버전 ${update.version}이 나왔습니다. 지금 업데이트할까요?\n받아서 설치한 뒤 OpenCam을 다시 켭니다.`, "업데이트"))) return;
  try {
    notify("새 버전을 받는 중입니다…");
    await update.downloadAndInstall();
    await relaunch();
  } catch (e) {
    await alertAsk(`업데이트하지 못했습니다: ${e}`);
  }
}
