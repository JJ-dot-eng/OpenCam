// `npm run tauri build`로 만든 결과를 GitHub 릴리스에 올릴 파일로 묶는다.
// 서명 열쇠를 넣고 빌드해야 업데이트용 .sig가 생긴다:
//   TAURI_SIGNING_PRIVATE_KEY=<열쇠 파일 경로> TAURI_SIGNING_PRIVATE_KEY_PASSWORD= npm run tauri build
//   node scripts/package-release.mjs [릴리스 설명 파일]
// 결과: src-tauri/target/release/publish/ 에 설치 파일, 포터블 zip, SHA256SUMS.txt, latest.json
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const REPO = "JJ-dot-eng/OpenCam";
const version = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8")).version;
const release = join(root, "src-tauri", "target", "release");
const setupName = `OpenCam_${version}_x64-setup.exe`;
const zipName = `OpenCam_${version}_x64_portable.zip`;
const setup = join(release, "bundle", "nsis", setupName);
if (!existsSync(setup) || !existsSync(`${setup}.sig`)) {
  throw new Error(`${setupName}과 .sig가 없습니다. 서명 열쇠를 넣고 npm run tauri build를 먼저 하세요.`);
}

const out = join(release, "publish");
rmSync(out, { recursive: true, force: true });
const stage = join(out, "stage", "OpenCam");
mkdirSync(stage, { recursive: true });

// 포터블: 실행 파일과 라이선스 문서를 OpenCam 폴더 하나에 담는다
copyFileSync(join(release, "opencam.exe"), join(stage, "OpenCam.exe"));
copyFileSync(join(root, "LICENSE"), join(stage, "LICENSE.txt"));
copyFileSync(join(root, "THIRD_PARTY_NOTICES.md"), join(stage, "THIRD_PARTY_NOTICES.md"));
copyFileSync(join(root, "THIRD_PARTY_LICENSES.txt"), join(stage, "THIRD_PARTY_LICENSES.txt"));
copyFileSync(join(root, "src-tauri", "models", "LICENSE-ESPCN.txt"), join(stage, "LICENSE-ESPCN.txt"));
writeFileSync(join(stage, "README.txt"), readFileSync(join(root, "scripts", "portable-README.txt"), "utf8").replace("{version}", version));
// Windows에 들어 있는 bsdtar로 zip을 만든다 (Git Bash의 GNU tar는 zip을 못 만든다)
const tar = join(process.env.SystemRoot || "C:\\Windows", "System32", "tar.exe");
execFileSync(tar, ["-a", "-cf", join(out, zipName), "-C", join(out, "stage"), "OpenCam"]);
rmSync(join(out, "stage"), { recursive: true, force: true });
copyFileSync(setup, join(out, setupName));

const sha = (f) => createHash("sha256").update(readFileSync(join(out, f))).digest("hex");
writeFileSync(join(out, "SHA256SUMS.txt"), [setupName, zipName].map((f) => `${sha(f)}  ${f}`).join("\n") + "\n");

// 설치 버전의 자동 업데이트가 읽는 파일 (릴리스의 latest/download/latest.json)
const notesFile = process.argv[2];
writeFileSync(
  join(out, "latest.json"),
  JSON.stringify(
    {
      version,
      notes: notesFile ? readFileSync(notesFile, "utf8").trim() : "",
      pub_date: new Date().toISOString(),
      platforms: {
        "windows-x86_64": {
          signature: readFileSync(`${setup}.sig`, "utf8").trim(),
          url: `https://github.com/${REPO}/releases/download/v${version}/${setupName}`,
        },
      },
    },
    null,
    2,
  ) + "\n",
);
console.log(`publish/: ${setupName}, ${zipName}, SHA256SUMS.txt, latest.json`);
